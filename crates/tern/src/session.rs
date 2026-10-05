//! One SSH session shown in one terminal: remote output feeds the terminal, keystrokes go to
//! the remote, and login questions are answered in the terminal itself (see `login.rs`).

use gpui::{App, AppContext, Context, Entity, Subscription, Task, Window};
use tern_ssh::{ConnectSpec, InputError, SecretString, SessionEvent, SessionHandle, TermSize};
use tern_term::{Terminal, TerminalEvent, TerminalView};

use crate::login::Login;
use crate::runtime::SshRuntime;
use tern_term::TerminalTheme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Closed,
}

pub struct Session {
    pub status: Status,
    flow: Option<vault::Flow>,
    /// The vault entry the open server question would answer.
    asking: Option<tern_vault::Key>,
    /// What the user typed for that question, offered for saving once connected.
    typed: Option<(tern_vault::Key, SecretString)>,
    /// Entries already answered from the vault on this connection.
    tried: Vec<tern_vault::Key>,
    pub view: Entity<TerminalView>,
    terminal: Entity<Terminal>,
    spec: ConnectSpec,
    size: TermSize,
    handle: SessionHandle,
    login: Option<Login>,
    _session_events: Task<()>,
    _terminal_events: Subscription,
}

impl Session {
    pub fn open(
        spec: ConnectSpec,
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
            let (handle, task) = Self::dial(&spec, size, cx);
            Self {
                status: Status::Connecting,
                flow: None,
                asking: None,
                typed: None,
                tried: Vec::new(),
                view,
                terminal,
                spec,
                size,
                handle,
                login: None,
                _session_events: task,
                _terminal_events: subscription,
            }
        })
    }

    /// Starts a connection and the task that feeds its events back. Dropping the task stops
    /// the feed, so a reconnect never hears from the connection it replaced.
    fn dial(
        spec: &ConnectSpec,
        size: TermSize,
        cx: &mut Context<Self>,
    ) -> (SessionHandle, Task<()>) {
        tracing::info!(host = %spec.host, port = spec.port, "session_open");
        let (handle, events) = tern_ssh::connect(spec.clone(), size, &SshRuntime::handle(cx));
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |s, cx| s.on_session_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        (handle, task)
    }

    pub fn reconnect(&mut self, cx: &mut Context<Self>) {
        self.show(b"\r\n", cx);
        let (handle, task) = Self::dial(&self.spec, self.size, cx);
        self.handle = handle;
        self._session_events = task;
        self.status = Status::Connecting;
        cx.notify();
    }

    fn on_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Data(bytes) => self.show(&bytes, cx),
            SessionEvent::Prompt(prompt) => self.on_prompt(prompt, cx),
            SessionEvent::Connected => {
                self.status = Status::Connected;
                self.offer_save(cx);
                cx.notify();
            }
            SessionEvent::Closed { exit_status, error } => {
                self.status = Status::Closed;
                self.login = None;
                self.flow = None;
                self.asking = None;
                self.typed = None;
                self.tried.clear();
                let reason = match (error, exit_status) {
                    (Some(e), _) => e,
                    (None, Some(code)) => format!("exit status {code}"),
                    (None, None) => "closed".into(),
                };
                self.show(
                    format!(
                        "\r\n\x1b[2m[connection closed: {reason}]\x1b[0m\r\n\
                         \x1b[2mPress Enter to reconnect\x1b[0m\r\n"
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
            TerminalEvent::Output(bytes) if self.status == Status::Closed => {
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
                if let Err(e) = self.handle.resize(size) {
                    tracing::debug!(error = %e, "session_resize_skipped");
                }
            }
            TerminalEvent::TitleChanged(_) | TerminalEvent::Bell => {}
        }
    }

    fn send(&self, bytes: Vec<u8>) {
        match self.handle.write(bytes) {
            Ok(()) | Err(InputError::Closed) => {}
            Err(InputError::Busy) => tracing::warn!("session_input_dropped_busy"),
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
fn wants_reconnect(input: &[u8]) -> bool {
    input.contains(&b'\r')
}

#[cfg(test)]
mod tests {
    use super::wants_reconnect;

    #[test]
    fn only_enter_reconnects() {
        assert!(wants_reconnect(b"\r"));
        assert!(!wants_reconnect(b"a"));
        assert!(!wants_reconnect(b"\x1b[A"));
        assert!(!wants_reconnect(b"\n"));
    }
}
