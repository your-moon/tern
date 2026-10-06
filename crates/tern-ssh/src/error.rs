use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("empty connection target")]
    EmptyTarget,
    #[error("invalid connection target: {0}")]
    InvalidTarget(String),
    #[error("invalid port in connection target: {0}")]
    InvalidPort(String),
    #[error("cannot determine local user name; use user@host")]
    NoLocalUser,
}

/// Why input could not be queued for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    /// The remote has stopped reading and the input queue is full; retry later.
    #[error("session is not accepting input right now")]
    Busy,
    /// The session has ended.
    #[error("session closed")]
    Closed,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Why a session ended before or during setup; rendered into `SessionEvent::Closed::error`.
/// Messages never include secrets.
#[derive(Debug)]
pub(crate) enum Failure {
    HostKeyChanged {
        line: usize,
    },
    HostKeyRejected,
    HostKeyUnsupported,
    HostKeyCheck(String),
    AuthFailed,
    AuthCancelled,
    Proxy(String),
    ConnectTimeout,
    Transport(String),
    /// A socket error, kept whole so the caller can word it with the host it was reaching.
    Io(std::io::Error),
    UiGone,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::HostKeyChanged { line } => write!(
                f,
                "host key changed: the key presented by the server does not match known_hosts line {line}; \
                 connection refused (possible man-in-the-middle attack)"
            ),
            Failure::HostKeyRejected => write!(f, "host key rejected by user"),
            Failure::HostKeyUnsupported => write!(f, "host certificates are not supported"),
            Failure::HostKeyCheck(e) => write!(f, "host key check failed: {e}"),
            Failure::AuthFailed => write!(
                f,
                "permission denied: the server accepted none of the passwords or keys tried"
            ),
            Failure::AuthCancelled => write!(f, "authentication cancelled"),
            Failure::Proxy(e) => write!(f, "ProxyCommand failed: {e}"),
            Failure::ConnectTimeout => write!(f, "no answer within 15 seconds"),
            Failure::Transport(e) => write!(f, "{e}"),
            Failure::Io(e) => write!(f, "{e}"),
            Failure::UiGone => write!(f, "event receiver dropped"),
        }
    }
}

impl From<russh::Error> for Failure {
    fn from(e: russh::Error) -> Self {
        match e {
            russh::Error::IO(io) => Failure::Io(io),
            other => Failure::Transport(other.to_string()),
        }
    }
}
