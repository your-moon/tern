//! SSH client sessions for tern.
//!
//! Events are delivered over an `async-channel` and prompts are answered through
//! `futures::channel::oneshot` senders, so the consumer (GPUI) does not need a tokio
//! context; the session itself runs on the tokio runtime handle passed to [`connect`].
//!
//! A host whose name resolves to a `ProxyCommand` in `~/.ssh/config` (matched by
//! [`ConnectSpec::host`]) is reached through that command, as OpenSSH does.
//! Set `TERN_KNOWN_HOSTS` to use a different known_hosts file (tests).

mod authn;
mod config;
mod error;
mod hostkey;
mod outbox;
mod session;

use std::path::PathBuf;

use futures::channel::oneshot;
use tokio::sync::mpsc;

pub use config::load_ssh_config_hosts;
pub use error::{Error, InputError, Result};
pub use secrecy::{self, ExposeSecret, SecretString};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEntry {
    pub alias: String,
    pub host_name: String,
    pub port: u16,
    pub user: Option<String>,
    pub identity_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSpec {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub identity_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

/// A question for the user. Dropping the reply sender counts as cancelling.
/// Secrets are [`SecretString`] (zeroized on drop, redacted in `Debug`).
#[derive(Debug)]
pub enum Prompt {
    Password {
        user: String,
        host: String,
        reply: oneshot::Sender<Option<SecretString>>,
    },
    KeyPassphrase {
        path: PathBuf,
        reply: oneshot::Sender<Option<SecretString>>,
    },
    UnknownHostKey {
        host: String,
        port: u16,
        algorithm: String,
        /// e.g. `SHA256:abc...`
        fingerprint_sha256: String,
        reply: oneshot::Sender<bool>,
    },
}

#[derive(Debug)]
pub enum SessionEvent {
    Connected,
    Data(Vec<u8>),
    Prompt(Prompt),
    Closed {
        exit_status: Option<u32>,
        error: Option<String>,
    },
}

/// Commands queued from the UI to one session. Keystrokes arrive at human speed and a paste
/// is one command, so a short queue only fills when the remote has stopped reading.
const COMMAND_QUEUE: usize = 64;

/// Sends input to one session. Cheap to clone; every clone drives the same session.
#[derive(Debug, Clone)]
#[must_use = "dropping every SessionHandle closes the session"]
pub struct SessionHandle {
    tx: mpsc::Sender<session::Command>,
}

impl SessionHandle {
    /// Queues bytes for the remote (keystrokes, paste, terminal replies).
    ///
    /// # Errors
    ///
    /// [`InputError::Busy`] when the remote has stopped reading and the queue is full;
    /// [`InputError::Closed`] once the session has ended.
    pub fn write(&self, bytes: Vec<u8>) -> std::result::Result<(), InputError> {
        self.send(session::Command::Write(bytes))
    }

    /// Tells the remote the terminal changed size (`window-change`).
    ///
    /// # Errors
    ///
    /// Same as [`Self::write`].
    pub fn resize(&self, size: TermSize) -> std::result::Result<(), InputError> {
        self.send(session::Command::Resize(size))
    }

    /// Asks the session to end; a [`SessionEvent::Closed`] follows.
    pub fn close(&self) {
        // A full or closed queue both mean the session is already on its way out.
        let _ = self.tx.try_send(session::Command::Close);
    }

    fn send(&self, cmd: session::Command) -> std::result::Result<(), InputError> {
        self.tx.try_send(cmd).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => InputError::Busy,
            mpsc::error::TrySendError::Closed(_) => InputError::Closed,
        })
    }
}

/// Starts a session on `rt`. The first event is a [`Prompt`] or [`SessionEvent::Connected`];
/// the last is always [`SessionEvent::Closed`].
///
/// Memory per session is bounded: see `outbox.rs` and `docs/perf.md`.
#[must_use = "the session runs only while its events are received"]
pub fn connect(
    spec: ConnectSpec,
    size: TermSize,
    rt: &tokio::runtime::Handle,
) -> (SessionHandle, async_channel::Receiver<SessionEvent>) {
    let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
    let (etx, erx) = async_channel::bounded(outbox::EVENT_QUEUE);
    rt.spawn(session::run(spec, size, rx, etx));
    (SessionHandle { tx }, erx)
}
