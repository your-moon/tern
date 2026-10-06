//! Port forwarding over a session: `-L` (local), `-R` (remote) and `-D` (dynamic SOCKS5).
//!
//! Local and dynamic forwards listen on this machine and open one direct-tcpip channel per
//! accepted connection (RFC 4254 section 7.2); a remote forward asks the server to listen
//! (`tcpip-forward`) and connects each forwarded-tcpip channel it opens to a local address.
//! Semantics follow `man ssh` (`-L -R -D`) and `man ssh_config` (`LocalForward` and friends).
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use russh::client::{Handle, Msg};
use russh::{Channel, ChannelOpenFailure};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::error::{Error, ForwardError, Result};
use crate::hostkey::Handler;
use crate::{session, socks};

/// Where a forward listens when the config or the caller names no address: this machine only.
const LOOPBACK: &str = "127.0.0.1";
/// Where a remote forward listens on the server when none is named, as OpenSSH does.
const SERVER_LOOPBACK: &str = "localhost";

/// One port forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forward {
    /// `-L`: connections to `bind_host:bind_port` here are carried to `dest_host:dest_port`
    /// as seen from the server.
    Local {
        bind_host: String,
        bind_port: u16,
        dest_host: String,
        dest_port: u16,
    },
    /// `-R`: the server listens on `bind_host:bind_port`; connections to it are carried back
    /// and made to `dest_host:dest_port` from this machine. `bind_port` 0 lets the server choose.
    Remote {
        bind_host: String,
        bind_port: u16,
        dest_host: String,
        dest_port: u16,
    },
    /// `-D`: a SOCKS5 proxy on `bind_host:bind_port`; each CONNECT goes out from the server.
    Dynamic { bind_host: String, bind_port: u16 },
}

impl fmt::Display for Forward {
    /// The `ssh` command-line form, e.g. `-L 127.0.0.1:8080:db:5432`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = |s: &str| {
            if s.contains(':') {
                format!("[{s}]")
            } else {
                s.to_string()
            }
        };
        match self {
            Forward::Local {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            } => write!(
                f,
                "-L {}:{bind_port}:{}:{dest_port}",
                h(bind_host),
                h(dest_host)
            ),
            Forward::Remote {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            } => write!(
                f,
                "-R {}:{bind_port}:{}:{dest_port}",
                h(bind_host),
                h(dest_host)
            ),
            Forward::Dynamic {
                bind_host,
                bind_port,
            } => write!(f, "-D {}:{bind_port}", h(bind_host)),
        }
    }
}

impl Forward {
    /// Parses the arguments of a `LocalForward` line: `[bind_address:]port host:hostport`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidForward`] when a part is missing or a port is not 1-65535 (0 is allowed
    /// for the listening port).
    pub fn parse_local(args: &str) -> Result<Self> {
        let (bind_host, bind_port, dest_host, dest_port) = listen_and_dest(args, LOOPBACK)?;
        Ok(Forward::Local {
            bind_host,
            bind_port,
            dest_host,
            dest_port,
        })
    }

    /// Parses the arguments of a `RemoteForward` line: `[bind_address:]port host:hostport`.
    ///
    /// # Errors
    ///
    /// As [`Self::parse_local`].
    pub fn parse_remote(args: &str) -> Result<Self> {
        let (bind_host, bind_port, dest_host, dest_port) = listen_and_dest(args, SERVER_LOOPBACK)?;
        Ok(Forward::Remote {
            bind_host,
            bind_port,
            dest_host,
            dest_port,
        })
    }

    /// Parses the arguments of a `DynamicForward` line: `[bind_address:]port`.
    ///
    /// # Errors
    ///
    /// As [`Self::parse_local`].
    pub fn parse_dynamic(args: &str) -> Result<Self> {
        let (bind_host, bind_port) = parse_listen(args.trim(), LOOPBACK)
            .ok_or_else(|| Error::InvalidForward(args.trim().to_string()))?;
        Ok(Forward::Dynamic {
            bind_host,
            bind_port,
        })
    }
}

fn listen_and_dest(args: &str, default_bind: &str) -> Result<(String, u16, String, u16)> {
    let bad = || Error::InvalidForward(args.trim().to_string());
    let mut parts = args.split_whitespace();
    let (Some(listen), Some(dest), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(bad());
    };
    let (bind_host, bind_port) = parse_listen(listen, default_bind).ok_or_else(bad)?;
    let (dest_host, dest_port) = split_host_port(dest).ok_or_else(bad)?;
    if dest_port == 0 {
        return Err(bad());
    }
    Ok((bind_host, bind_port, dest_host, dest_port))
}

/// `[bind:]port`, with `[v6]` brackets and `*` for all interfaces.
fn parse_listen(s: &str, default_bind: &str) -> Option<(String, u16)> {
    if let Ok(port) = s.parse::<u16>() {
        return Some((default_bind.to_string(), port));
    }
    let (host, port) = split_host_port(s)?;
    let host = if host == "*" { "0.0.0.0".into() } else { host };
    Some((host, port))
}

/// `host:port` or `[v6]:port`.
fn split_host_port(s: &str) -> Option<(String, u16)> {
    let (host, port) = match s.strip_prefix('[') {
        Some(rest) => {
            let (host, after) = rest.split_once(']')?;
            (host, after.strip_prefix(':')?)
        }
        None => s.rsplit_once(':')?,
    };
    if host.is_empty() || (!s.starts_with('[') && host.contains(':')) {
        return None;
    }
    Some((host.to_string(), port.parse().ok()?))
}

/// An active forward, as listed by [`SessionHandle::forwards`](crate::SessionHandle::forwards).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardInfo {
    pub id: u64,
    pub forward: Forward,
    /// The port actually listening: differs from the requested one only when that was 0.
    pub bound_port: u16,
}

/// Stops one forward. Cheap to clone; dropping it leaves the forward running.
#[derive(Debug, Clone)]
pub struct ForwardHandle {
    pub(crate) info: ForwardInfo,
    pub(crate) registry: Arc<Registry>,
    pub(crate) cmds: mpsc::Sender<session::Command>,
}

impl ForwardHandle {
    #[must_use]
    pub fn info(&self) -> &ForwardInfo {
        &self.info
    }

    /// Stops listening. Connections already carried finish on their own. Does nothing if the
    /// forward has already stopped or the session has ended.
    pub fn stop(&self) {
        let Some(entry) = self.registry.remove(self.info.id) else {
            return;
        };
        if let Some(abort) = entry.abort {
            abort.abort();
        }
        if let Forward::Remote { bind_host, .. } = &self.info.forward {
            self.registry
                .remote_targets
                .remove(bind_host, self.info.bound_port);
            // A full or closed queue both mean the session is on its way out, which ends it.
            let _ = self.cmds.try_send(session::Command::CancelRemote {
                host: bind_host.clone(),
                port: self.info.bound_port,
            });
        }
    }
}

struct Entry {
    info: ForwardInfo,
    abort: Option<AbortHandle>,
}

/// The forwards of one session, shared by its handle, its task and its russh handler.
#[derive(Default)]
pub(crate) struct Registry {
    next: AtomicU64,
    entries: Mutex<Vec<Entry>>,
    pub remote_targets: RemoteTargets,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Registry")
    }
}

impl Registry {
    fn add(&self, forward: Forward, bound_port: u16, abort: Option<AbortHandle>) -> ForwardInfo {
        let info = ForwardInfo {
            id: self.next.fetch_add(1, Ordering::Relaxed),
            forward,
            bound_port,
        };
        if let Ok(mut e) = self.entries.lock() {
            e.push(Entry {
                info: info.clone(),
                abort,
            });
        }
        info
    }

    fn remove(&self, id: u64) -> Option<Entry> {
        let mut e = self.entries.lock().ok()?;
        let at = e.iter().position(|x| x.info.id == id)?;
        Some(e.remove(at))
    }

    pub fn list(&self) -> Vec<ForwardInfo> {
        self.entries
            .lock()
            .map(|e| e.iter().map(|x| x.info.clone()).collect())
            .unwrap_or_default()
    }

    /// Ends every listener; called when the session is over.
    pub fn clear(&self) {
        if let Ok(mut e) = self.entries.lock() {
            for x in e.drain(..) {
                if let Some(a) = x.abort {
                    a.abort();
                }
            }
        }
    }
}

/// What each remote forward is to be connected to, keyed by where the server listens.
#[derive(Clone, Default)]
pub(crate) struct RemoteTargets(Arc<Mutex<Vec<RemoteTarget>>>);

struct RemoteTarget {
    listen_host: String,
    listen_port: u16,
    dest_host: String,
    dest_port: u16,
}

impl RemoteTargets {
    fn add(&self, listen_host: &str, listen_port: u16, dest_host: &str, dest_port: u16) {
        if let Ok(mut t) = self.0.lock() {
            t.push(RemoteTarget {
                listen_host: listen_host.to_string(),
                listen_port,
                dest_host: dest_host.to_string(),
                dest_port,
            });
        }
    }

    fn remove(&self, listen_host: &str, listen_port: u16) {
        if let Ok(mut t) = self.0.lock() {
            t.retain(|x| !(x.listen_port == listen_port && x.listen_host == listen_host));
        }
    }

    /// The destination for a channel the server opened for `address:port`. Servers do not
    /// always echo the address as requested (`localhost` comes back as `127.0.0.1`), so a
    /// port that only one forward uses is enough.
    pub fn lookup(&self, address: &str, port: u32) -> Option<(String, u16)> {
        let t = self.0.lock().ok()?;
        let on_port: Vec<_> = t
            .iter()
            .filter(|x| u32::from(x.listen_port) == port)
            .collect();
        let pick = on_port
            .iter()
            .find(|x| x.listen_host.eq_ignore_ascii_case(address))
            .or(if on_port.len() == 1 {
                on_port.first()
            } else {
                None
            })?;
        Some((pick.dest_host.clone(), pick.dest_port))
    }
}

/// Starts `forward` on a logged-in session; returns its listing.
pub(crate) async fn start(
    session: &Arc<Handle<Handler>>,
    registry: &Arc<Registry>,
    forward: Forward,
) -> std::result::Result<ForwardInfo, ForwardError> {
    match forward.clone() {
        Forward::Local {
            bind_host,
            bind_port,
            dest_host,
            dest_port,
        } => {
            let (listener, port) = listen(&bind_host, bind_port).await?;
            let session = session.clone();
            let task = tokio::spawn(async move {
                while let Ok((tcp, peer)) = listener.accept().await {
                    tokio::spawn(carry(
                        session.clone(),
                        tcp,
                        peer,
                        dest_host.clone(),
                        dest_port,
                    ));
                }
            });
            Ok(registry.add(forward, port, Some(task.abort_handle())))
        }
        Forward::Dynamic {
            bind_host,
            bind_port,
        } => {
            let (listener, port) = listen(&bind_host, bind_port).await?;
            let session = session.clone();
            let task = tokio::spawn(async move {
                while let Ok((tcp, peer)) = listener.accept().await {
                    tokio::spawn(socks_connection(session.clone(), tcp, peer));
                }
            });
            Ok(registry.add(forward, port, Some(task.abort_handle())))
        }
        Forward::Remote {
            bind_host,
            bind_port,
            dest_host,
            dest_port,
        } => {
            let granted = session
                .tcpip_forward(bind_host.clone(), u32::from(bind_port))
                .await
                .map_err(|_| ForwardError::Refused(format!("{bind_host}:{bind_port}")))?;
            let port = if bind_port == 0 {
                u16::try_from(granted).unwrap_or(0)
            } else {
                bind_port
            };
            registry
                .remote_targets
                .add(&bind_host, port, &dest_host, dest_port);
            Ok(registry.add(forward, port, None))
        }
    }
}

async fn listen(host: &str, port: u16) -> std::result::Result<(TcpListener, u16), ForwardError> {
    let fail = |e: io::Error| ForwardError::Listen {
        addr: format!("{host}:{port}"),
        reason: match e.kind() {
            io::ErrorKind::AddrInUse => "the port is already in use".into(),
            io::ErrorKind::PermissionDenied => "permission denied".into(),
            _ => e.to_string(),
        },
    };
    let listener = TcpListener::bind((host, port)).await.map_err(fail)?;
    let bound = listener.local_addr().map_err(fail)?.port();
    Ok((listener, bound))
}

/// One accepted connection of a local forward.
async fn carry(
    session: Arc<Handle<Handler>>,
    mut tcp: TcpStream,
    peer: SocketAddr,
    host: String,
    port: u16,
) {
    match open(&session, &host, port, peer).await {
        Ok(ch) => splice(&mut tcp, ch).await,
        Err(e) => tracing::debug!(host = %host, port, error = %e, "ssh_forward_open_failed"),
    }
}

/// One accepted connection of a dynamic forward.
async fn socks_connection(session: Arc<Handle<Handler>>, mut tcp: TcpStream, peer: SocketAddr) {
    let (host, port) = match socks::accept(&mut tcp).await {
        Ok(dest) => dest,
        Err(e) => {
            tracing::debug!(error = %e, "ssh_socks_refused");
            return;
        }
    };
    match open(&session, &host, port, peer).await {
        Ok(ch) => {
            if socks::reply(&mut tcp, socks::REP_OK).await.is_ok() {
                splice(&mut tcp, ch).await;
            }
        }
        Err(e) => {
            tracing::debug!(host = %host, port, error = %e, "ssh_forward_open_failed");
            let code = match e {
                russh::Error::ChannelOpenFailure(_) => socks::REP_CONNECTION_REFUSED,
                _ => socks::REP_GENERAL_FAILURE,
            };
            let _ = socks::reply(&mut tcp, code).await;
        }
    }
}

async fn open(
    session: &Handle<Handler>,
    host: &str,
    port: u16,
    peer: SocketAddr,
) -> std::result::Result<Channel<Msg>, russh::Error> {
    session
        .channel_open_direct_tcpip(
            host,
            u32::from(port),
            peer.ip().to_string(),
            u32::from(peer.port()),
        )
        .await
}

async fn splice(tcp: &mut TcpStream, ch: Channel<Msg>) {
    let mut ch = ch.into_stream();
    let _ = tokio::io::copy_bidirectional(tcp, &mut ch).await;
}

/// A forwarded-tcpip channel the server opened: connect it to the local destination.
pub(crate) async fn bridge_remote(ch: Channel<Msg>, host: String, port: u16) {
    match TcpStream::connect((host.as_str(), port)).await {
        Ok(mut tcp) => splice(&mut tcp, ch).await,
        Err(e) => tracing::debug!(host = %host, port, error = %e, "ssh_forward_connect_failed"),
    }
}

/// Why a server-opened channel was refused; used by the russh handler.
pub(crate) const NOT_FORWARDED: ChannelOpenFailure = ChannelOpenFailure::AdministrativelyProhibited;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn local(args: &str) -> (String, u16, String, u16) {
        match Forward::parse_local(args).unwrap() {
            Forward::Local {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
            } => (bind_host, bind_port, dest_host, dest_port),
            other => panic!("not local: {other:?}"),
        }
    }

    #[test]
    fn bare_port_binds_loopback() {
        assert_eq!(
            local("8080 localhost:80"),
            ("127.0.0.1".into(), 8080, "localhost".into(), 80)
        );
    }

    #[test]
    fn explicit_bind_address_is_kept() {
        assert_eq!(
            local("127.0.0.1:8080 db:5432"),
            ("127.0.0.1".into(), 8080, "db".into(), 5432)
        );
        assert_eq!(
            local("*:8080 db:1"),
            ("0.0.0.0".into(), 8080, "db".into(), 1)
        );
        assert_eq!(
            local("192.168.1.5:9000 db.internal:22"),
            ("192.168.1.5".into(), 9000, "db.internal".into(), 22)
        );
    }

    #[test]
    fn ipv6_needs_and_loses_its_brackets() {
        assert_eq!(
            local("[::1]:8081 [fe80::2]:80"),
            ("::1".into(), 8081, "fe80::2".into(), 80)
        );
        assert_eq!(
            local("8082 [::1]:80"),
            ("127.0.0.1".into(), 8082, "::1".into(), 80)
        );
    }

    #[test]
    fn rejects_what_ssh_would() {
        for bad in [
            "",
            "8080",
            "8080 db",
            "8080 db:0",
            "8080 db:99999",
            "http db:80",
            "8080 db:80 extra",
            "::1:8080 db:80",
            "[::1 db:80",
            "8080 :80",
        ] {
            assert!(Forward::parse_local(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn remote_defaults_to_the_servers_loopback_and_allows_port_zero() {
        let f = Forward::parse_remote("9000 localhost:3000").unwrap();
        assert_eq!(
            f,
            Forward::Remote {
                bind_host: "localhost".into(),
                bind_port: 9000,
                dest_host: "localhost".into(),
                dest_port: 3000
            }
        );
        let Forward::Remote { bind_port, .. } = Forward::parse_remote("0 h:1").unwrap() else {
            panic!()
        };
        assert_eq!(bind_port, 0);
    }

    #[test]
    fn dynamic_forms() {
        let d = |s| Forward::parse_dynamic(s).unwrap();
        assert_eq!(
            d("1080"),
            Forward::Dynamic {
                bind_host: "127.0.0.1".into(),
                bind_port: 1080
            }
        );
        assert_eq!(
            d("[::1]:1080"),
            Forward::Dynamic {
                bind_host: "::1".into(),
                bind_port: 1080
            }
        );
        assert!(Forward::parse_dynamic("db:80 x").is_err());
        assert!(Forward::parse_dynamic("").is_err());
    }

    #[test]
    fn displays_as_the_ssh_flag() {
        let f = Forward::parse_local("[::1]:8081 db:80").unwrap();
        assert_eq!(f.to_string(), "-L [::1]:8081:db:80");
        assert_eq!(
            Forward::parse_dynamic("1080").unwrap().to_string(),
            "-D 127.0.0.1:1080"
        );
    }

    #[test]
    fn remote_lookup_prefers_the_exact_address_and_tolerates_a_renamed_one() {
        let t = RemoteTargets::default();
        t.add("localhost", 9000, "a", 1);
        assert_eq!(t.lookup("127.0.0.1", 9000), Some(("a".into(), 1)));
        assert_eq!(t.lookup("localhost", 9001), None);
        t.add("0.0.0.0", 9000, "b", 2);
        assert_eq!(t.lookup("0.0.0.0", 9000), Some(("b".into(), 2)));
        assert_eq!(t.lookup("127.0.0.1", 9000), None);
        t.remove("0.0.0.0", 9000);
        assert_eq!(t.lookup("127.0.0.1", 9000), Some(("a".into(), 1)));
    }
}
