// Adapted from russh russh/examples/client_open_direct_tcpip.rs (Apache-2.0): direct-tcpip
// channel, then a handshake over its stream, repeated once per hop.
//! ProxyJump: each hop is a full SSH login, and the next connection runs inside a direct-tcpip
//! channel of the one before, as `ssh -J` does (russh `channel_open_direct_tcpip`, then a new
//! handshake over `Channel::into_stream`, after russh `examples/client_open_direct_tcpip.rs`).
use russh::client::{self, Handle};
use russh::{Channel, client::Msg};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::authn::Authenticator;
use crate::config;
use crate::disconnect::Cause;
use crate::error::{Error, Failure};
use crate::forward::RemoteTargets;
use crate::hostkey::Handler;
use crate::session::{CONNECT_TIMEOUT, client_config, connect_failure};
use crate::{ConnectSpec, SessionEvent};

/// A byte stream both ways; russh's channel stream type is not public.
pub(crate) trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}

/// The logged-in jump hosts (kept alive by being held) and a stream to the final target.
pub(crate) struct Chain {
    pub hops: Vec<Handle<Handler>>,
    pub stream: Box<dyn Duplex>,
}

/// Logs in to every hop in order, then opens a channel from the last one to `spec.host:port`.
/// Host keys and credentials are checked per hop; prompts name the hop they are about.
pub(crate) async fn open(
    spec: &ConnectSpec,
    events: &async_channel::Sender<SessionEvent>,
) -> Result<Chain, Failure> {
    let mut hops: Vec<Handle<Handler>> = Vec::new();
    for hop in &spec.proxy_jump {
        if hop.port == 0 {
            return Err(Failure::BadJump(hop.host.clone()));
        }
        let label = format!("{}:{}", hop.host, hop.port);
        let wrap = |f: Failure| Failure::Jump(label.clone(), Box::new(f));
        let handler = Handler {
            host: hop.host.clone(),
            port: hop.port,
            known_hosts: spec.known_hosts.clone(),
            events: events.clone(),
            cause: Cause::default(),
            remote: RemoteTargets::default(),
            agent_socket: None,
        };
        let cfg = client_config(spec, &hop.host, hop.port);
        let connecting = async {
            match hops.last() {
                None => client::connect(cfg, (hop.host.as_str(), hop.port), handler).await,
                Some(prev) => {
                    let ch = open_channel(prev, &hop.host, hop.port).await?;
                    client::connect_stream(cfg, ch.into_stream(), handler).await
                }
            }
        };
        let mut session = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
            .await
            .map_err(|_| wrap(Failure::ConnectTimeout))?
            .map_err(|e| wrap(connect_failure(e, &hop.host, hop.port)))?;
        let user = hop
            .user
            .clone()
            .or_else(config::local_user)
            .ok_or_else(|| wrap(Failure::Transport(Error::NoLocalUser.to_string())))?;
        Authenticator::new(
            &mut session,
            &user,
            &hop.host,
            hop.identity_files.clone(),
            events,
        )
        .run()
        .await
        .map_err(wrap)?;
        tracing::info!(host = %hop.host, port = hop.port, "ssh_jump_connected");
        hops.push(session);
    }
    let last = hops.last().ok_or(Failure::UiGone)?;
    let label = format!("{}:{}", spec.proxy_jump.last().map_or("", |h| &h.host), {
        spec.proxy_jump.last().map_or(0, |h| h.port)
    });
    let ch = open_channel(last, &spec.host, spec.port)
        .await
        .map_err(|f| Failure::Jump(label, Box::new(f)))?;
    Ok(Chain {
        hops,
        stream: Box::new(ch.into_stream()),
    })
}

async fn open_channel(
    from: &Handle<Handler>,
    host: &str,
    port: u16,
) -> Result<Channel<Msg>, Failure> {
    from.channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0)
        .await
        .map_err(|e| match e {
            russh::Error::ChannelOpenFailure(_) => {
                Failure::Transport(format!("it could not open a connection to {host}:{port}"))
            }
            other => other.into(),
        })
}
