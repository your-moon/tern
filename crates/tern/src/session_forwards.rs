//! Port forwards of one session: the ones the connection lists start once it is connected, and
//! more can be added or stopped while it is open. What was added stays in the spec, so an
//! automatic reconnect brings it back; what was stopped is dropped from it.

use gpui::{Context, EventEmitter};
use tern_ssh::{Forward, ForwardInfo};

use super::Session;

/// Something the shell should tell the person about, without the session knowing how.
pub enum SessionNote {
    ForwardFailed {
        forward: String,
        error: String,
    },
    /// The remote is asking for a password and one could be filled: show the shortcut.
    PasswordPrompt,
    /// A line for a toast.
    Message(String),
    /// A line for a toast that is an error.
    Problem(String),
}

impl EventEmitter<SessionNote> for Session {}

impl Session {
    /// Starts every forward the connection lists. Called once the shell is up.
    pub(super) fn start_spec_forwards(&mut self, cx: &mut Context<Self>) {
        for forward in self.link.spec.forwards.clone() {
            self.run_forward(forward, false, cx);
        }
    }

    /// Adds a forward to the live connection (and to its reconnects).
    pub fn add_forward(&mut self, forward: Forward, cx: &mut Context<Self>) {
        if !self.link.spec.forwards.contains(&forward) {
            self.link.spec.forwards.push(forward.clone());
        }
        self.run_forward(forward, true, cx);
    }

    fn run_forward(&mut self, forward: Forward, added: bool, cx: &mut Context<Self>) {
        let Some(handle) = self.link.handle.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let started = handle.start_forward(forward.clone()).await;
            let _ = this.update(cx, |s, cx| {
                match started {
                    Ok(h) => s.forward_handles.push(h),
                    Err(e) => {
                        tracing::warn!(forward = %forward, error = %e, "forward_failed");
                        if added {
                            s.link.spec.forwards.retain(|f| *f != forward);
                        }
                        cx.emit(SessionNote::ForwardFailed {
                            forward: forward.to_string(),
                            error: e.to_string(),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stops one forward and forgets it, so a reconnect does not revive it.
    pub fn stop_forward(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(at) = self.forward_handles.iter().position(|h| h.info().id == id) else {
            return;
        };
        let handle = self.forward_handles.remove(at);
        handle.stop();
        let gone = handle.info().forward.clone();
        self.link.spec.forwards.retain(|f| *f != gone);
        cx.notify();
    }

    /// The forwards running now.
    pub fn active_forwards(&self) -> Vec<ForwardInfo> {
        self.forward_handles
            .iter()
            .map(|h| h.info().clone())
            .collect()
    }

    /// A connection that ended took its forwards with it.
    pub(super) fn forget_forwards(&mut self) {
        self.forward_handles.clear();
    }
}
