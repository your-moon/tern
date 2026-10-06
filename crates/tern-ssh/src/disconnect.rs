//! Why a session ended, in a form the app can act on (auto-reconnect only for network causes).
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::error::Failure;

/// Why a session ended. Carried by [`SessionEvent::Closed`](crate::SessionEvent::Closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disconnect {
    /// The remote shell ended with this exit status, e.g. the user typed `exit`.
    Exited(u32),
    /// The remote shell was killed by a signal.
    Signaled(String),
    /// This side closed the session ([`SessionHandle::close`](crate::SessionHandle::close)).
    Local,
    /// The server stopped answering keep-alives, or the connection went silent.
    Timeout,
    /// The connection was reset or broke mid-session (network drop, sleep, roaming).
    Reset,
    /// The server closed the connection, or the shell channel, without an exit status.
    ServerClosed,
    /// The server could not be reached while connecting (refused, no route, no answer).
    Unreachable,
    /// Setup failed for a reason retrying will not fix: host key, login, proxy, bad request.
    Failed,
}

impl Disconnect {
    /// True for causes a reconnect can cure. False after `exit`, a local close, or a refused
    /// login, where reconnecting would be wrong or pointless.
    #[must_use]
    pub fn is_network(&self) -> bool {
        matches!(
            self,
            Disconnect::Timeout
                | Disconnect::Reset
                | Disconnect::ServerClosed
                | Disconnect::Unreachable
        )
    }
}

/// What the russh connection task learned about its own death, shared with the session loop.
/// russh reports a dead link only to the handler, and the channel can go quiet just before.
#[derive(Debug, Clone)]
pub(crate) struct Cause(Arc<watch::Sender<Option<Disconnect>>>);

impl Default for Cause {
    fn default() -> Self {
        Cause(Arc::new(watch::channel(None).0))
    }
}

impl Cause {
    /// Records the first cause only; later ones are consequences of it.
    pub fn set(&self, d: Disconnect) {
        self.0.send_if_modified(|slot| {
            let first = slot.is_none();
            if first {
                *slot = Some(d);
            }
            first
        });
    }

    /// The cause, waiting up to `grace` for russh to report it.
    pub async fn wait(&self, grace: Duration) -> Option<Disconnect> {
        let mut rx = self.0.subscribe();
        let seen = tokio::time::timeout(grace, rx.wait_for(Option::is_some)).await;
        match seen {
            Ok(Ok(v)) => v.clone(),
            _ => None,
        }
    }
}

/// Classifies a failure. `connected` is true once the SSH handshake had finished.
pub(crate) fn classify(f: &Failure, connected: bool) -> Disconnect {
    match f {
        Failure::KeepaliveTimeout => Disconnect::Timeout,
        Failure::Unreachable(_) | Failure::ConnectTimeout => Disconnect::Unreachable,
        Failure::Io(e) => match e.kind() {
            ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof => Disconnect::Reset,
            ErrorKind::TimedOut if connected => Disconnect::Timeout,
            _ if connected => Disconnect::Reset,
            _ => Disconnect::Unreachable,
        },
        Failure::Transport(_) if connected => Disconnect::ServerClosed,
        _ => Disconnect::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Error;

    #[test]
    fn only_network_causes_are_reconnectable() {
        for d in [
            Disconnect::Timeout,
            Disconnect::Reset,
            Disconnect::ServerClosed,
            Disconnect::Unreachable,
        ] {
            assert!(d.is_network(), "{d:?}");
        }
        for d in [
            Disconnect::Exited(0),
            Disconnect::Exited(1),
            Disconnect::Signaled("KILL".into()),
            Disconnect::Local,
            Disconnect::Failed,
        ] {
            assert!(!d.is_network(), "{d:?}");
        }
    }

    #[test]
    fn failures_map_to_their_cause() {
        let io = |k| Failure::Io(Error::from(k));
        assert_eq!(
            classify(&Failure::KeepaliveTimeout, true),
            Disconnect::Timeout
        );
        assert_eq!(
            classify(&io(ErrorKind::ConnectionReset), true),
            Disconnect::Reset
        );
        assert_eq!(
            classify(&io(ErrorKind::ConnectionRefused), false),
            Disconnect::Unreachable
        );
        assert_eq!(
            classify(&io(ErrorKind::ConnectionRefused), true),
            Disconnect::Reset
        );
        assert_eq!(classify(&Failure::AuthFailed, true), Disconnect::Failed);
        assert_eq!(
            classify(&Failure::HostKeyRejected, false),
            Disconnect::Failed
        );
        assert_eq!(
            classify(&Failure::Transport("x".into()), false),
            Disconnect::Failed
        );
        assert_eq!(
            classify(&Failure::Transport("x".into()), true),
            Disconnect::ServerClosed
        );
        assert_eq!(
            classify(&Failure::ConnectTimeout, false),
            Disconnect::Unreachable
        );
    }
}
