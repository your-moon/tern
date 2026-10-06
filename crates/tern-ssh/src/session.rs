// Adapted from russh russh/examples/client_exec_interactive.rs (Apache-2.0) for the
// PTY/shell/event loop, and from CrabPort crabport-ssh/src/backend.rs (Apache-2.0)
// for window_change handling and protocol-level keepalive.
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::oneshot;
use russh::client::{self, Handle};
use russh::{ChannelMsg, Disconnect as SshDisconnect, Preferred};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::authn::Authenticator;
use crate::config;
use crate::disconnect::{Cause, classify};
use crate::error::Failure;
use crate::forward::{self, Registry};
use crate::hostkey::{Handler, known_algorithms};
use crate::jump;
use crate::outbox::Outbox;
use crate::{ConnectSpec, Disconnect, Forward, ForwardError, ForwardInfo, SessionEvent, TermSize};

pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long to wait for russh to say why a connection that went quiet has died.
const CAUSE_GRACE: Duration = Duration::from_millis(500);
/// After exit-status, how long to wait for the server to close the channel.
const EXIT_GRACE: Duration = Duration::from_secs(2);
const TERM: &str = "xterm-256color";
/// Unread SSH messages russh buffers per channel before it stops reading the socket.
/// russh's default is 100; at up to 32 KiB a packet that is 3 MiB per session.
const CHANNEL_BUFFER: usize = 16;

#[derive(Debug)]
pub(crate) enum Command {
    Write(Vec<u8>),
    Resize(TermSize),
    /// Start a forward; the session answers on `reply`.
    Forward {
        forward: Forward,
        reply: oneshot::Sender<Result<ForwardInfo, ForwardError>>,
    },
    /// Tell the server to stop a remote forward.
    CancelRemote {
        host: String,
        port: u16,
    },
    Close,
}

struct Outcome {
    exit_status: Option<u32>,
    error: Option<String>,
    reason: Disconnect,
}

pub(crate) async fn run(
    spec: ConnectSpec,
    size: TermSize,
    mut cmds: mpsc::Receiver<Command>,
    events: async_channel::Sender<SessionEvent>,
    forwards: Arc<Registry>,
) {
    let cause = Cause::default();
    let mut connected = false;
    let outcome = match run_inner(
        &spec,
        size,
        &mut cmds,
        &events,
        &cause,
        &mut connected,
        &forwards,
    )
    .await
    {
        Ok(o) => o,
        Err(f) => {
            tracing::warn!(host = %spec.host, port = spec.port, error = %f, "ssh_session_failed");
            // What russh saw on the dead link is more exact than the error it surfaced as.
            let reason = match connected {
                true => match cause.wait(CAUSE_GRACE).await {
                    Some(c) => c,
                    None => classify(&f, true),
                },
                false => classify(&f, false),
            };
            Outcome {
                exit_status: None,
                error: Some(f.to_string()),
                reason,
            }
        }
    };
    forwards.clear();
    tracing::info!(host = %spec.host, port = spec.port, exit_status = ?outcome.exit_status, reason = ?outcome.reason, "ssh_closed");
    let _ = events
        .send(SessionEvent::Closed {
            exit_status: outcome.exit_status,
            error: outcome.error,
            reason: outcome.reason,
        })
        .await;
}

/// The proxy's first stderr line, which says why it failed when it does.
#[derive(Clone, Default)]
struct ProxyReason(Arc<std::sync::Mutex<Option<String>>>);

impl ProxyReason {
    fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|s| s.clone())
    }
}

/// ProxyCommand transport: the child's stdio is the byte stream.
fn spawn_proxy(
    cmd: &str,
) -> Result<
    (
        Child,
        impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        ProxyReason,
    ),
    Failure,
> {
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Failure::Proxy(e.to_string()))?;
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(Failure::Proxy("stdio unavailable".into()));
    };
    let reason = ProxyReason::default();
    let first = reason.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(proxy_stderr = %line, "ssh_proxy_stderr");
            // The first line is usually the error; what follows tends to be usage text.
            if let Ok(mut slot) = first.0.lock()
                && slot.is_none()
                && !line.trim().is_empty()
            {
                *slot = Some(line.trim().to_owned());
            }
        }
    });
    Ok((child, tokio::io::join(stdout, stdin), reason))
}

/// The russh settings for one connection: keep-alive from the spec, and the host key types
/// already in known_hosts for `host:port` asked for first.
pub(crate) fn client_config(spec: &ConnectSpec, host: &str, port: u16) -> Arc<client::Config> {
    let mut preferred = Preferred::default();
    let known = known_algorithms(host, port, spec.known_hosts.as_deref());
    if !known.is_empty() {
        let mut keys = known;
        let rest: Vec<_> = preferred
            .key
            .iter()
            .filter(|a| !keys.contains(a))
            .cloned()
            .collect();
        keys.extend(rest);
        preferred.key = Cow::Owned(keys);
    }
    Arc::new(client::Config {
        // OpenSSH: an interval of 0 turns keep-alives off; a count of 0 never gives up (russh
        // reads 0 the same way).
        keepalive_interval: Some(spec.server_alive_interval).filter(|d| !d.is_zero()),
        keepalive_max: spec.server_alive_count_max as usize,
        nodelay: true,
        channel_buffer_size: CHANNEL_BUFFER,
        preferred,
        ..Default::default()
    })
}

async fn run_inner(
    spec: &ConnectSpec,
    size: TermSize,
    cmds: &mut mpsc::Receiver<Command>,
    events: &async_channel::Sender<SessionEvent>,
    cause: &Cause,
    handshaken: &mut bool,
    forwards: &Arc<Registry>,
) -> Result<Outcome, Failure> {
    let handler = Handler {
        host: spec.host.clone(),
        port: spec.port,
        known_hosts: spec.known_hosts.clone(),
        events: events.clone(),
        cause: cause.clone(),
        remote: forwards.remote_targets.clone(),
    };

    let cfg = client_config(spec, &spec.host, spec.port);

    tracing::info!(host = %spec.host, port = spec.port, user = %spec.user, "ssh_connecting");
    // Jump hosts are logged in to first; their connections stay open for the whole session.
    let mut _hops = Vec::new();
    let mut jump_stream = None;
    if !spec.proxy_jump.is_empty() {
        let chain = jump::open(spec, events).await?;
        _hops = chain.hops;
        jump_stream = Some(chain.stream);
    }
    let proxy = spec.proxy_command.as_deref();
    // Held for the whole session so the proxy process lives as long as the connection.
    let mut _proxy_child: Option<Child> = None;
    let mut proxy_reason: Option<ProxyReason> = None;
    let connecting = async {
        if let Some(stream) = jump_stream.take() {
            return Ok(client::connect_stream(cfg, stream, handler).await);
        }
        match proxy {
            Some(cmd) => {
                let cmd = config::expand_proxy_command(cmd, &spec.host, spec.port, &spec.user);
                tracing::debug!("ssh_proxy_command_start");
                let (child, stream, reason) = spawn_proxy(&cmd)?;
                _proxy_child = Some(child);
                proxy_reason = Some(reason);
                Ok(client::connect_stream(cfg, stream, handler).await)
            }
            None => Ok::<_, Failure>(
                client::connect(cfg, (spec.host.as_str(), spec.port), handler).await,
            ),
        }
    };
    let connected = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
        .await
        .map_err(|_| Failure::ConnectTimeout)??;
    let mut session = match connected {
        Ok(s) => s,
        Err(e) => {
            return Err(proxy_exit(&mut _proxy_child, proxy_reason.as_ref())
                .unwrap_or_else(|| connect_failure(e, &spec.host, spec.port)));
        }
    };

    let identity_files = spec.identity_files.clone();
    Authenticator::new(&mut session, &spec.user, &spec.host, identity_files, events)
        .with_memory_keys(spec.memory_keys.clone())
        .run()
        .await?;

    *handshaken = true;
    let session = Arc::new(session);
    let mut channel = session.channel_open_session().await?;
    channel
        .request_pty(
            false,
            TERM,
            size.cols.into(),
            size.rows.into(),
            size.pixel_width.into(),
            size.pixel_height.into(),
            &[],
        )
        .await?;
    channel.request_shell(true).await?;
    events
        .send(SessionEvent::Connected)
        .await
        .map_err(|_| Failure::UiGone)?;
    tracing::info!(host = %spec.host, port = spec.port, "ssh_connected");

    let mut exit_status = None;
    let mut error = None;
    let mut signal = None;
    let mut deadline: Option<Instant> = None;
    let mut closing = false;
    let mut outbox = Outbox::new(events.clone());
    loop {
        tokio::select! {
            cmd = cmds.recv(), if !closing => match cmd {
                Some(Command::Write(bytes)) => channel.data_bytes(bytes).await?,
                Some(Command::Resize(s)) => {
                    channel.window_change(s.cols.into(), s.rows.into(), s.pixel_width.into(), s.pixel_height.into()).await?
                }
                Some(Command::Forward { forward, reply }) => {
                    let (session, forwards) = (session.clone(), forwards.clone());
                    tokio::spawn(async move {
                        let _ = reply.send(forward::start(&session, &forwards, forward).await);
                    });
                }
                Some(Command::CancelRemote { host, port }) => {
                    let session = session.clone();
                    tokio::spawn(async move {
                        let _ = session.cancel_tcpip_forward(host, u32::from(port)).await;
                    });
                }
                Some(Command::Close) | None => {
                    closing = true;
                    let _ = channel.close().await;
                    deadline = Some(Instant::now() + EXIT_GRACE);
                }
            },
            delivered = outbox.progress(), if outbox.has_work() => {
                if !delivered { break; }
            }
            // Not reading while the outbox is full is the backpressure: see outbox.rs.
            msg = channel.wait(), if outbox.wants_input() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    outbox.push(&data);
                }
                Some(ChannelMsg::ExitStatus { exit_status: code }) => {
                    exit_status = Some(code);
                    deadline.get_or_insert_with(|| Instant::now() + EXIT_GRACE);
                }
                Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                    error = Some(format!("terminated by signal {signal_name:?}"));
                    signal = Some(format!("{signal_name:?}"));
                    deadline.get_or_insert_with(|| Instant::now() + EXIT_GRACE);
                }
                Some(ChannelMsg::Failure) => {
                    error = Some("server refused the shell request".to_string());
                    break;
                }
                Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
            () = async { match deadline { Some(d) => tokio::time::sleep_until(d).await, None => std::future::pending().await } } => break,
        }
    }
    // Output that arrived just before exit (a final prompt, an error message) still reaches the UI.
    outbox.flush().await;
    let reason = match (exit_status, signal) {
        (Some(code), _) => Disconnect::Exited(code),
        (None, Some(name)) => Disconnect::Signaled(name),
        (None, None) if closing => Disconnect::Local,
        // The shell channel ended with no exit status: the link died or the server hung up.
        (None, None) => cause
            .wait(CAUSE_GRACE)
            .await
            .unwrap_or(Disconnect::ServerClosed),
    };
    disconnect(&session).await;
    Ok(Outcome {
        exit_status,
        error,
        reason,
    })
}

/// A proxy that died during the handshake explains the failure better than russh's
/// "Disconnected".
fn proxy_exit(child: &mut Option<Child>, reason: Option<&ProxyReason>) -> Option<Failure> {
    let status = child.as_mut()?.try_wait().ok()??;
    Some(Failure::Proxy(
        reason
            .and_then(ProxyReason::get)
            .unwrap_or_else(|| status.to_string()),
    ))
}

/// A connect error in words a person can act on, naming where tern tried to go.
pub(crate) fn connect_failure(e: Failure, host: &str, port: u16) -> Failure {
    match e {
        Failure::Io(io) => Failure::Unreachable(describe_io(&io, host, port)),
        other => other,
    }
}

pub(crate) fn describe_io(e: &std::io::Error, host: &str, port: u16) -> String {
    use std::io::ErrorKind as K;
    let text = e.to_string();
    match e.kind() {
        K::ConnectionRefused => {
            format!("{host}:{port} refused the connection (is an SSH server running there?)")
        }
        K::TimedOut => format!("{host}:{port} did not answer"),
        K::HostUnreachable | K::NetworkUnreachable => format!("no route to {host}"),
        K::ConnectionReset => format!("{host}:{port} closed the connection"),
        _ if text.contains("lookup address") || text.contains("nodename nor servname") => {
            format!("could not find host {host}")
        }
        _ => text,
    }
}

async fn disconnect(session: &Handle<Handler>) {
    let _ = session
        .disconnect(SshDisconnect::ByApplication, "", "en")
        .await;
}

#[cfg(test)]
mod describe_tests {
    use super::describe_io;
    use std::io::{Error, ErrorKind};

    #[test]
    fn common_failures_read_as_sentences_naming_the_target() {
        let refused = Error::from(ErrorKind::ConnectionRefused);
        assert_eq!(
            describe_io(&refused, "10.0.0.5", 2222),
            "10.0.0.5:2222 refused the connection (is an SSH server running there?)"
        );
        let dns =
            Error::other("failed to lookup address information: nodename nor servname provided");
        assert_eq!(
            describe_io(&dns, "nope.example", 22),
            "could not find host nope.example"
        );
        let odd = Error::other("something else");
        assert_eq!(describe_io(&odd, "h", 22), "something else");
    }
}
