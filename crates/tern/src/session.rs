//! One SSH session shown in one terminal: remote output feeds the terminal, keystrokes go to
//! the remote, and login questions are answered in the terminal itself (see `login.rs`).

use gpui::{App, AppContext, Context, Entity, Subscription, Task, WeakEntity, Window};
use tern_ssh::{
    ConnectSpec, InputError, MemoryKey, SecretString, SessionEvent, SessionHandle, TermSize,
};
use tern_term::{Terminal, TerminalEvent, TerminalView};

use crate::keeper::Keeper;
use crate::login::Login;
use crate::runtime::SshRuntime;
use crate::session_log::{self, SessionLog};
use tern_term::TerminalTheme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Restored from the last run and not dialled yet: Enter connects.
    Idle,
    Connecting,
    Connected,
    Closed,
}

/// What a new session runs.
#[derive(Debug, Clone)]
pub enum Launch {
    Ssh(ConnectSpec),
    /// A connection that waits for Enter before it dials, so reopening many tabs at launch
    /// does not hit every server at once.
    SshIdle(ConnectSpec),
}

/// Where the session connects, and the live connection once there is one.
struct Link {
    spec: ConnectSpec,
    handle: Option<SessionHandle>,
    /// Names of vault keys offered at login. They are looked up each time the session dials,
    /// so the spec never holds key material between connections.
    vault_keys: Vec<String>,
}

impl Status {
    /// Nothing is running; Enter starts it.
    pub fn is_dormant(&self) -> bool {
        matches!(self, Status::Idle | Status::Closed)
    }
}

pub struct Session {
    pub status: Status,
    /// When the current connection came up, and when it ended; the status line shows the
    /// time between (or since).
    connected_at: Option<std::time::Instant>,
    ended_at: Option<std::time::Instant>,
    flow: Option<vault::Flow>,
    /// The vault passphrase is being asked for only so the connection can then dial.
    connect_after_unlock: bool,
    /// The vault entry the open server question would answer.
    asking: Option<tern_vault::Key>,
    /// What the user typed for that question, offered for saving once connected.
    typed: Option<(tern_vault::Key, SecretString)>,
    /// Entries already answered from the vault on this connection.
    tried: Vec<tern_vault::Key>,
    /// Secrets the user was already asked for on this connection.
    asked: Vec<tern_vault::Key>,
    /// When the remote last wrote, and whether that ended a line: the save offer waits for a
    /// quiet moment so it is not interleaved with the login banner.
    last_output: std::time::Instant,
    ends_line: bool,
    pub view: Entity<TerminalView>,
    terminal: Entity<Terminal>,
    link: Link,
    /// The tab's name, for the log file.
    name: String,
    log: Option<SessionLog>,
    /// Sessions that get a copy of what is typed here (broadcast input).
    mirrors: Vec<WeakEntity<Session>>,
    /// Every connection of this session is logged, as the settings ask.
    auto_log: bool,
    size: TermSize,
    login: Option<Login>,
    _session_events: Task<()>,
    _terminal_events: Subscription,
}

impl Session {
    pub fn open(
        launch: Launch,
        vault_keys: Vec<String>,
        name: String,
        auto_log: bool,
        theme: TerminalTheme,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let size = TermSize {
            cols: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        };
        let terminal = cx.new(|_| Terminal::new(size.cols, size.rows));
        let view = cx.new(|cx| TerminalView::new(terminal.clone(), theme, window, cx));
        cx.new(|cx| {
            let subscription = cx.subscribe(&terminal, |this: &mut Self, _, event, cx| {
                this.on_terminal_event(event, cx);
            });
            let idle = matches!(launch, Launch::SshIdle(_));
            let (Launch::Ssh(spec) | Launch::SshIdle(spec)) = launch;
            let mut link = Link {
                spec,
                handle: None,
                vault_keys,
            };
            // A locked vault is unlocked before dialling, so its keys can be offered.
            let unlock_first = !idle && Self::keys_need_unlock(&link, cx);
            let task = if idle || unlock_first {
                Task::ready(())
            } else {
                Self::start(&mut link, size, cx)
            };
            let this = Self {
                status: if idle {
                    Status::Idle
                } else {
                    Status::Connecting
                },
                connected_at: None,
                ended_at: None,
                flow: None,
                connect_after_unlock: false,
                asking: None,
                typed: None,
                tried: Vec::new(),
                asked: Vec::new(),
                last_output: std::time::Instant::now(),
                ends_line: true,
                view,
                terminal,
                link,
                name,
                log: None,
                mirrors: Vec::new(),
                auto_log,
                size,
                login: None,
                _session_events: task,
                _terminal_events: subscription,
            };
            if idle {
                this.show(b"\x1b[2mPress Enter to connect\x1b[0m\r\n", cx);
            }
            let mut this = this;
            if auto_log && !idle {
                this.start_auto_log();
            }
            if unlock_first {
                this.ask_vault_before_dialling(cx);
            }
            this
        })
    }

    /// Starts the connection and the task that feeds its events back. Dropping the task stops
    /// the feed, so a reconnect never hears from the connection it replaced.
    fn start(link: &mut Link, size: TermSize, cx: &mut Context<Self>) -> Task<()> {
        tracing::info!(host = %link.spec.host, port = link.spec.port, "session_open");
        let mut dial = link.spec.clone();
        for name in &link.vault_keys {
            let key = tern_vault::Key::SshKey { name: name.clone() };
            if let Some(openssh) = Keeper::lookup(&key, cx) {
                dial.memory_keys.push(MemoryKey {
                    name: name.clone(),
                    openssh,
                });
            }
        }
        let (handle, events) = tern_ssh::connect(dial, size, &SshRuntime::handle(cx));
        link.handle = Some(handle);
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |s, cx| s.on_session_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    /// The server this session dials.
    pub(super) fn host(&self) -> &str {
        &self.link.spec.host
    }

    pub(super) fn port(&self) -> u16 {
        self.link.spec.port
    }

    /// Sends a copy of everything typed in this session to `mirrors`.
    pub fn set_mirrors(&mut self, mirrors: Vec<WeakEntity<Session>>) {
        self.mirrors = mirrors;
    }

    /// What was typed here also goes to the mirrors, except while a login question is open: a
    /// password typed for this server must not reach another one.
    fn mirror_typed(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if self.login.is_some() || !self.accepts_input() {
            return;
        }
        self.mirrors.retain(|m| m.upgrade().is_some());
        for mirror in &self.mirrors {
            let bytes = bytes.to_vec();
            let _ = mirror.update(cx, |s, _| s.send_if_live(bytes));
        }
    }

    /// Connected, with no login question waiting for an answer.
    fn accepts_input(&self) -> bool {
        self.status == Status::Connected && self.login.is_none()
    }

    /// Input from another tab: delivered only while this session is live, never to a closed
    /// or not-yet-connected one.
    fn send_if_live(&self, bytes: Vec<u8>) {
        if self.accepts_input() {
            self.send(bytes);
        }
    }

    pub fn is_logging(&self) -> bool {
        self.log.is_some()
    }

    /// Starts writing this session's output to a new file under `~/Library/Logs/tern/sessions`.
    ///
    /// # Errors
    ///
    /// When the file cannot be created.
    pub fn start_logging(&mut self) -> std::io::Result<std::path::PathBuf> {
        let dir = session_log::directory()
            .ok_or_else(|| std::io::Error::other("no home directory for the log"))?;
        let log = SessionLog::create(&dir, &self.name, std::time::SystemTime::now())?;
        let path = log.path().to_owned();
        tracing::info!(path = %path.display(), "session_log_started");
        self.log = Some(log);
        Ok(path)
    }

    /// The settings' "log every session": a failure is logged, never in the way of the tab.
    fn start_auto_log(&mut self) {
        if let Err(e) = self.start_logging() {
            tracing::warn!(error = %e, "session_log_start_failed");
        }
    }

    /// Stops logging and returns the file it wrote.
    pub fn stop_logging(&mut self) -> Option<std::path::PathBuf> {
        self.log.take().map(|log| log.path().to_owned())
    }

    /// What opens another session just like this one: the same server.
    pub fn launch_again(&self) -> Launch {
        Launch::Ssh(self.link.spec.clone())
    }

    pub fn reconnect(&mut self, cx: &mut Context<Self>) {
        self.show(b"\r\n", cx);
        if self.auto_log && self.log.is_none() {
            self.start_auto_log();
        }
        self.status = Status::Connecting;
        self.connected_at = None;
        self.ended_at = None;
        if Self::keys_need_unlock(&self.link, cx) {
            self.ask_vault_before_dialling(cx);
        } else {
            self.dial(cx);
        }
        cx.notify();
    }

    /// Starts the connection now.
    fn dial(&mut self, cx: &mut Context<Self>) {
        self._session_events = Self::start(&mut self.link, self.size, cx);
    }

    /// The connection names vault keys, and the vault is locked.
    fn keys_need_unlock(link: &Link, cx: &App) -> bool {
        !link.vault_keys.is_empty() && Keeper::locked(cx)
    }

    /// `user@host:port` for the status line.
    pub fn label(&self) -> String {
        let spec = &self.link.spec;
        crate::statusline::host_label(&spec.user, &spec.host, spec.port)
    }

    /// How long this connection has been up (frozen when it closed); `None` before it connects.
    pub fn elapsed(&self) -> Option<std::time::Duration> {
        let start = self.connected_at?;
        Some(self.ended_at.unwrap_or_else(std::time::Instant::now) - start)
    }

    fn on_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Data(bytes) => {
                self.last_output = std::time::Instant::now();
                self.ends_line = bytes.last().is_some_and(|b| *b == b'\n');
                self.record(&bytes);
                self.show(&bytes, cx);
            }
            SessionEvent::Prompt(prompt) => self.on_prompt(prompt, cx),
            SessionEvent::Connected => {
                self.status = Status::Connected;
                self.connected_at = Some(std::time::Instant::now());
                self.ended_at = None;
                self.offer_save(cx);
                cx.notify();
            }
            SessionEvent::Closed {
                exit_status, error, ..
            } => {
                self.status = Status::Closed;
                self.ended_at = Some(std::time::Instant::now());
                self.login = None;
                self.flow = None;
                self.asking = None;
                self.typed = None;
                self.tried.clear();
                self.asked.clear();
                let reason = match (error, exit_status) {
                    (Some(e), _) => e,
                    (None, Some(code)) => format!("exit status {code}"),
                    (None, None) => "closed".into(),
                };
                let next = match reason.starts_with("host key changed") {
                    // Reconnecting only fails again; say how to drop the old key, but leave that
                    // step to the user, because a changed key is also what an attack looks like.
                    true => format!(
                        "If the server was reinstalled, remove its old key and reconnect:\r\n  {}",
                        forget_key_command(&self.link.spec)
                    ),
                    false => "Press Enter to reconnect".into(),
                };
                self.show(
                    format!(
                        "\r\n\x1b[2m[connection closed: {reason}]\x1b[0m\r\n\
                         \x1b[2m{next}\x1b[0m\r\n"
                    )
                    .as_bytes(),
                    cx,
                );
                cx.notify();
            }
        }
    }

    fn on_terminal_event(&mut self, event: &TerminalEvent, cx: &mut Context<Self>) {
        match event {
            TerminalEvent::Output(bytes) if self.status.is_dormant() => {
                if wants_reconnect(bytes) {
                    self.reconnect(cx);
                }
            }
            TerminalEvent::Output(bytes) => match self.login.as_mut() {
                Some(login) => {
                    let step = login.input(bytes);
                    let answer = step.done.then(|| login.take_answer()).flatten();
                    if step.done {
                        self.login = None;
                    }
                    self.show(&step.display, cx);
                    if step.done {
                        self.answered(answer, cx);
                    }
                }
                None => self.send(bytes.clone()),
            },
            TerminalEvent::Resized {
                cols,
                rows,
                pixel_width,
                pixel_height,
            } => {
                let size = TermSize {
                    cols: *cols,
                    rows: *rows,
                    pixel_width: *pixel_width,
                    pixel_height: *pixel_height,
                };
                self.size = size;
                if let Some(handle) = &self.link.handle
                    && let Err(e) = handle.resize(size)
                {
                    tracing::debug!(error = %e, "session_resize_skipped");
                }
            }
            TerminalEvent::Typed(bytes) => self.mirror_typed(bytes, cx),
            TerminalEvent::TitleChanged(_) | TerminalEvent::Bell => {}
        }
    }

    pub fn terminal(&self) -> Entity<Terminal> {
        self.terminal.clone()
    }

    /// Types `text` into the remote as the keyboard would: a newline is Enter (CR), so a
    /// snippet runs only when it ends with one. False when nothing could take it (not
    /// connected, or a login question is open and would swallow it).
    pub fn insert_text(&self, text: &str) -> bool {
        if self.status != Status::Connected || self.login.is_some() {
            return false;
        }
        self.send(text.replace("\r\n", "\r").replace('\n', "\r").into_bytes());
        true
    }

    /// Output goes to the log when there is one; a write that fails ends the logging.
    fn record(&mut self, bytes: &[u8]) {
        if let Some(log) = self.log.as_mut()
            && let Err(e) = log.write(bytes)
        {
            tracing::warn!(error = %e, "session_log_write_failed");
            self.log = None;
        }
    }

    fn send(&self, bytes: Vec<u8>) {
        if let Some(handle) = &self.link.handle {
            match handle.write(bytes) {
                Ok(()) | Err(InputError::Closed) => {}
                Err(InputError::Busy) => tracing::warn!("session_input_dropped_busy"),
            }
        }
    }

    fn show(&self, bytes: &[u8], cx: &mut Context<Self>) {
        self.terminal.update(cx, |t, cx| t.feed(bytes, cx));
    }
}

#[path = "session_vault.rs"]
mod vault;

/// A closed tab reconnects on Enter only, so a stray keystroke into a dead tab does not dial
/// the server again. Enter arrives as CR from both the main and the keypad key.
/// The `ssh-keygen -R` line that forgets this server's key, in OpenSSH's `[host]:port` form.
fn forget_key_command(spec: &ConnectSpec) -> String {
    let host = match spec.port {
        22 => spec.host.clone(),
        port => format!("'[{}]:{port}'", spec.host),
    };
    match &spec.known_hosts {
        Some(path) => format!("ssh-keygen -R {host} -f '{}'", path.display()),
        None => format!("ssh-keygen -R {host}"),
    }
}

fn wants_reconnect(input: &[u8]) -> bool {
    input.contains(&b'\r')
}

#[cfg(test)]
mod tests {
    use super::{forget_key_command, wants_reconnect};
    use tern_ssh::ConnectSpec;

    #[test]
    fn forget_key_command_uses_openssh_host_forms() {
        let mut spec = ConnectSpec {
            host: "10.0.0.5".into(),
            port: 2222,
            user: "deploy".into(),
            identity_files: Vec::new(),
            proxy_command: None,
            known_hosts: None,
            memory_keys: Vec::new(),
        };
        assert_eq!(forget_key_command(&spec), "ssh-keygen -R '[10.0.0.5]:2222'");
        spec.port = 22;
        spec.known_hosts = Some("/tmp/kh".into());
        assert_eq!(
            forget_key_command(&spec),
            "ssh-keygen -R 10.0.0.5 -f '/tmp/kh'"
        );
    }

    #[test]
    fn only_enter_reconnects() {
        assert!(wants_reconnect(b"\r"));
        assert!(!wants_reconnect(b"a"));
        assert!(!wants_reconnect(b"\x1b[A"));
        assert!(!wants_reconnect(b"\n"));
    }
}
