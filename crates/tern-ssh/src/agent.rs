// Adapted from russh russh/examples/client_exec_interactive.rs (Apache-2.0) for the
// `agent_forward` request; the channel proxy follows `ssh(1)` agent forwarding (PROTOCOL.agent).
//! Agent forwarding: the server opens `auth-agent@openssh.com` channels, and each one is wired to
//! the local agent socket, so the remote side can sign with keys that never leave this machine.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use russh::Channel;
use russh::client::Msg;

/// The agent socket to forward for a spec that asks for forwarding: its own path, else
/// `$SSH_AUTH_SOCK`.
pub(crate) fn socket_for(wanted: bool, explicit: Option<&Path>) -> Option<PathBuf> {
    if !wanted {
        return None;
    }
    let found = explicit
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty());
    if found.is_none() {
        tracing::warn!("ssh_agent_forward_skipped: no SSH_AUTH_SOCK");
    }
    found
}

/// Reads `ForwardAgent`: `yes` uses `$SSH_AUTH_SOCK`, `no` is off, an absolute path or `~/` path
/// names the socket, and `$NAME` / `${NAME}` takes it from that variable.
pub(crate) fn parse_forward_agent(value: &str) -> (bool, Option<PathBuf>) {
    let v = value.trim();
    match v.to_ascii_lowercase().as_str() {
        "yes" | "true" => return (true, None),
        "no" | "false" | "" => return (false, None),
        _ => {}
    }
    if let Some(name) = v.strip_prefix('$') {
        let name = name.trim_start_matches('{').trim_end_matches('}');
        return match std::env::var_os(OsStr::new(name)) {
            Some(p) if !p.is_empty() => (true, Some(PathBuf::from(p))),
            _ => (false, None),
        };
    }
    if let Some(rest) = v.strip_prefix("~/") {
        return match crate::config::home_dir() {
            Some(h) => (true, Some(h.join(rest))),
            None => (false, None),
        };
    }
    if v.starts_with('/') {
        return (true, Some(PathBuf::from(v)));
    }
    (false, None)
}

/// Copies between a server-opened agent channel and the local agent until either ends.
#[cfg(unix)]
pub(crate) async fn bridge(channel: Channel<Msg>, socket: PathBuf) {
    match tokio::net::UnixStream::connect(&socket).await {
        Ok(mut agent) => {
            let mut ch = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut agent, &mut ch).await;
        }
        Err(e) => {
            tracing::debug!(socket = %socket.display(), error = %e, "ssh_agent_connect_failed");
            // Tell the server now, rather than leaving it waiting on a channel nobody serves.
            let _ = channel.close().await;
        }
    }
}

#[cfg(not(unix))]
pub(crate) async fn bridge(_channel: Channel<Msg>, _socket: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_agent_values() {
        assert_eq!(parse_forward_agent("yes"), (true, None));
        assert_eq!(parse_forward_agent("YES"), (true, None));
        assert_eq!(parse_forward_agent("no"), (false, None));
        assert_eq!(
            parse_forward_agent("/run/agent.sock"),
            (true, Some(PathBuf::from("/run/agent.sock")))
        );
        assert_eq!(parse_forward_agent("$TERN_SURELY_UNSET_VAR"), (false, None));
        assert_eq!(parse_forward_agent("maybe"), (false, None));
    }

    #[test]
    fn no_forwarding_means_no_socket() {
        assert_eq!(socket_for(false, Some(Path::new("/x"))), None);
        assert_eq!(
            socket_for(true, Some(Path::new("/x"))),
            Some(PathBuf::from("/x"))
        );
    }
}
