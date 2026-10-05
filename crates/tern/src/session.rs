//! One SSH session shown in one terminal: remote output feeds the terminal, keystrokes go to
//! the remote, and login questions are answered in the terminal itself (see `login.rs`).

use gpui::{App, AppContext, Context, Entity, Subscription, Window};
use tern_ssh::{ConnectSpec, InputError, SessionEvent, SessionHandle, TermSize};
use tern_term::{Terminal, TerminalEvent, TerminalView};

use crate::login::Login;
use crate::runtime::SshRuntime;
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Closed,
}

pub struct Session {
    pub status: Status,
    pub view: Entity<TerminalView>,
    terminal: Entity<Terminal>,
    handle: SessionHandle,
    login: Option<Login>,
    _terminal_events: Subscription,
}

impl Session {
    pub fn open(
        spec: ConnectSpec,
        theme: &Theme,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let size = TermSize {
            cols: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        };
        tracing::info!(host = %spec.host, port = spec.port, "session_open");
        let (handle, events) = tern_ssh::connect(spec, size, &SshRuntime::handle(cx));
        let terminal = cx.new(|_| Terminal::new(size.cols, size.rows));
        let view = cx.new(|cx| TerminalView::new(terminal.clone(), theme.terminal(), window, cx));
        cx.new(|cx| {
            let subscription = cx.subscribe(&terminal, |this: &mut Self, _, event, cx| {
                this.on_terminal_event(event, cx);
            });
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
            .detach();
            Self {
                status: Status::Connecting,
                view,
                terminal,
                handle,
                login: None,
                _terminal_events: subscription,
            }
        })
    }

    fn on_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Data(bytes) => self.show(&bytes, cx),
            SessionEvent::Prompt(prompt) => {
                let (login, question) = Login::start(prompt);
                self.login = Some(login);
                self.show(&question, cx);
            }
            SessionEvent::Connected => {
                self.status = Status::Connected;
                cx.notify();
            }
            SessionEvent::Closed { exit_status, error } => {
                self.status = Status::Closed;
                self.login = None;
                let reason = match (error, exit_status) {
                    (Some(e), _) => e,
                    (None, Some(code)) => format!("exit status {code}"),
                    (None, None) => "closed".into(),
                };
                self.show(
                    format!("\r\n\x1b[2m[connection closed: {reason}]\x1b[0m\r\n").as_bytes(),
                    cx,
                );
                cx.notify();
            }
        }
    }

    fn on_terminal_event(&mut self, event: &TerminalEvent, cx: &mut Context<Self>) {
        match event {
            TerminalEvent::Output(bytes) => match self.login.as_mut() {
                Some(login) => {
                    let step = login.input(bytes);
                    if step.done {
                        self.login = None;
                    }
                    self.show(&step.display, cx);
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
