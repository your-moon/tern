// Adapted from russh russh/examples/client_exec_interactive.rs (Apache-2.0) for the
// PTY/shell/event loop, and from CrabPort crabport-ssh/src/backend.rs (Apache-2.0)
// for window_change handling and protocol-level keepalive.
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle};
use russh::{ChannelMsg, Disconnect, Preferred};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::authn::Authenticator;
use crate::config;
use crate::error::Failure;
use crate::hostkey::{Handler, known_algorithms, known_hosts_override};
use crate::outbox::Outbox;
use crate::{ConnectSpec, SessionEvent, TermSize};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const KEEPALIVE: Duration = Duration::from_secs(30);
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
    Close,
}

struct Outcome {
    exit_status: Option<u32>,
    error: Option<String>,
}

pub(crate) async fn run(
    spec: ConnectSpec,
    size: TermSize,
    mut cmds: mpsc::Receiver<Command>,
    events: async_channel::Sender<SessionEvent>,
) {
    let outcome = match run_inner(&spec, size, &mut cmds, &events).await {
        Ok(o) => o,
        Err(f) => {
            tracing::warn!(host = %spec.host, port = spec.port, error = %f, "ssh_session_failed");
            Outcome {
                exit_status: None,
                error: Some(f.to_string()),
            }
        }
    };
    tracing::info!(host = %spec.host, port = spec.port, exit_status = ?outcome.exit_status, "ssh_closed");
    let _ = events
        .send(SessionEvent::Closed {
            exit_status: outcome.exit_status,
            error: outcome.error,
        })
        .await;
}

/// ProxyCommand transport: the child's stdio is the byte stream.
fn spawn_proxy(
    cmd: &str,
) -> Result<
    (
        Child,
        impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
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
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(proxy_stderr = %line, "ssh_proxy_stderr");
        }
    });
    Ok((child, tokio::io::join(stdout, stdin)))
}

async fn run_inner(
    spec: &ConnectSpec,
    size: TermSize,
    cmds: &mut mpsc::Receiver<Command>,
    events: &async_channel::Sender<SessionEvent>,
) -> Result<Outcome, Failure> {
    let known_hosts = known_hosts_override();
    let handler = Handler {
        host: spec.host.clone(),
        port: spec.port,
        known_hosts: known_hosts.clone(),
        events: events.clone(),
    };

    let mut preferred = Preferred::default();
    let known = known_algorithms(&spec.host, spec.port, known_hosts.as_deref());
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
    let cfg = Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE),
        nodelay: true,
        channel_buffer_size: CHANNEL_BUFFER,
        preferred,
        ..Default::default()
    });

    tracing::info!(host = %spec.host, port = spec.port, user = %spec.user, "ssh_connecting");
    let proxy = spec.proxy_command.as_deref();
    // Held for the whole session so the proxy process lives as long as the connection.
    let mut _proxy_child: Option<Child> = None;
    let connecting = async {
        match proxy {
            Some(cmd) => {
                let cmd = config::expand_proxy_command(cmd, &spec.host, spec.port, &spec.user);
                tracing::debug!("ssh_proxy_command_start");
                let (child, stream) = spawn_proxy(&cmd)?;
                _proxy_child = Some(child);
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
        Err(e) => return Err(proxy_exit(&mut _proxy_child).unwrap_or(e)),
    };

    let identity_files = spec.identity_files.clone();
    Authenticator::new(&mut session, &spec.user, &spec.host, identity_files, events)
        .run()
        .await?;

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
    disconnect(&session).await;
    Ok(Outcome { exit_status, error })
}

/// A proxy that died during the handshake explains the failure better than russh's
/// "Disconnected".
fn proxy_exit(child: &mut Option<Child>) -> Option<Failure> {
    let status = child.as_mut()?.try_wait().ok()??;
    Some(Failure::Proxy(status.to_string()))
}

async fn disconnect(session: &Handle<Handler>) {
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
}
