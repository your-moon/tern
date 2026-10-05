// Adapted from russh russh/src/keys/known_hosts.rs (Apache-2.0): uses its check/learn helpers,
// with the policy (match / prompt / hard-fail on changed key) layered on top.
use std::path::{Path, PathBuf};

use futures::channel::oneshot;
use russh::client;
use russh::keys::{Algorithm, HashAlg, PublicKey, PublicKeyOrCertificate, known_hosts};

use crate::error::Failure;
use crate::{Prompt, SessionEvent};

/// Env var overriding the known_hosts location (used by tests).
pub(crate) const KNOWN_HOSTS_ENV: &str = "TERN_KNOWN_HOSTS";

pub(crate) fn known_hosts_override() -> Option<PathBuf> {
    std::env::var_os(KNOWN_HOSTS_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Known,
    Unknown,
    Changed { line: usize },
    Error(String),
}

pub(crate) fn verdict(host: &str, port: u16, key: &PublicKey, path: Option<&Path>) -> Verdict {
    if let Some(p) = path
        && !p.exists()
    {
        return Verdict::Unknown;
    }
    let res = match path {
        Some(p) => known_hosts::check_known_hosts_path(host, port, key, p),
        None => known_hosts::check_known_hosts(host, port, key),
    };
    match res {
        Ok(true) => Verdict::Known,
        Ok(false) => Verdict::Unknown,
        Err(russh::keys::Error::KeyChanged { line }) => Verdict::Changed { line },
        Err(russh::keys::Error::IO(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            Verdict::Unknown
        }
        Err(e) => Verdict::Error(e.to_string()),
    }
}

/// Algorithms already recorded for this host, so the server is asked for those first
/// (otherwise a server offering a different key type would look "unknown", not "changed").
pub(crate) fn known_algorithms(host: &str, port: u16, path: Option<&Path>) -> Vec<Algorithm> {
    let keys = match path {
        Some(p) => known_hosts::known_host_keys_path(host, port, p),
        None => known_hosts::known_host_keys(host, port),
    };
    let mut out: Vec<Algorithm> = Vec::new();
    for (_, k) in keys.unwrap_or_default() {
        let a = k.algorithm();
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

pub(crate) struct Handler {
    pub host: String,
    pub port: u16,
    pub known_hosts: Option<PathBuf>,
    pub events: async_channel::Sender<SessionEvent>,
}

impl client::Handler for Handler {
    /// russh returns this from `connect` unchanged (it requires only `From<russh::Error>`),
    /// so a refused host key reaches the session as the specific [`Failure`].
    type Error = Failure;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            return Err(Failure::HostKeyUnsupported);
        };
        match verdict(&self.host, self.port, key, self.known_hosts.as_deref()) {
            Verdict::Known => {
                tracing::debug!(host = %self.host, port = self.port, "ssh_host_key_known");
                Ok(true)
            }
            Verdict::Changed { line } => {
                tracing::error!(host = %self.host, port = self.port, known_hosts_line = line, "ssh_host_key_changed");
                Err(Failure::HostKeyChanged { line })
            }
            Verdict::Error(e) => {
                tracing::error!(host = %self.host, port = self.port, error = %e, "ssh_host_key_check_failed");
                Err(Failure::HostKeyCheck(e))
            }
            Verdict::Unknown => {
                tracing::info!(host = %self.host, port = self.port, "ssh_host_key_unknown");
                let (tx, rx) = oneshot::channel();
                let prompt = Prompt::UnknownHostKey {
                    host: self.host.clone(),
                    port: self.port,
                    algorithm: key.algorithm().to_string(),
                    fingerprint_sha256: key.fingerprint(HashAlg::Sha256).to_string(),
                    reply: tx,
                };
                if self
                    .events
                    .send(SessionEvent::Prompt(prompt))
                    .await
                    .is_err()
                {
                    return Err(Failure::UiGone);
                }
                if !rx.await.unwrap_or(false) {
                    return Err(Failure::HostKeyRejected);
                }
                let learned = match self.known_hosts.as_deref() {
                    Some(p) => known_hosts::learn_known_hosts_path(&self.host, self.port, key, p),
                    None => known_hosts::learn_known_hosts(&self.host, self.port, key),
                };
                if let Err(e) = learned {
                    // Same as OpenSSH: the user accepted, so continue, but say we could not persist it.
                    tracing::warn!(host = %self.host, port = self.port, error = %e, "ssh_known_hosts_write_failed");
                }
                Ok(true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const KEY_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFgetNZuhJ8rf7QTXKOWMELOSgDO5tuRDWi5bRS9UNAC";
    const KEY_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHt6DH7o7tXSqrdZsggH9EARU3XRi2kqRpl9pHdUyBec";

    fn tmp(name: &str, body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("tern-ssh-{}-{name}", std::process::id()));
        std::fs::write(&p, body).unwrap();
        p
    }

    fn key(s: &str) -> PublicKey {
        PublicKey::from_openssh(s).unwrap()
    }

    #[test]
    fn matching_key_is_known() {
        let p = tmp("known", &format!("strong-b {KEY_A}\n"));
        assert_eq!(
            verdict("strong-b", 22, &key(KEY_A), Some(&p)),
            Verdict::Known
        );
    }

    #[test]
    fn different_key_for_same_host_is_changed() {
        let p = tmp("changed", &format!("other {KEY_B}\nstrong-b {KEY_B}\n"));
        assert_eq!(
            verdict("strong-b", 22, &key(KEY_A), Some(&p)),
            Verdict::Changed { line: 2 }
        );
    }

    #[test]
    fn unlisted_host_and_missing_file_are_unknown() {
        let p = tmp("unlisted", &format!("other {KEY_B}\n"));
        assert_eq!(
            verdict("strong-b", 22, &key(KEY_A), Some(&p)),
            Verdict::Unknown
        );
        let missing = std::env::temp_dir().join("tern-ssh-does-not-exist");
        assert_eq!(
            verdict("strong-b", 22, &key(KEY_A), Some(&missing)),
            Verdict::Unknown
        );
    }

    #[test]
    fn non_default_port_uses_bracket_form() {
        let p = tmp("port", &format!("[strong-b]:2222 {KEY_B}\n"));
        assert_eq!(
            verdict("strong-b", 2222, &key(KEY_A), Some(&p)),
            Verdict::Changed { line: 1 }
        );
        assert_eq!(
            verdict("strong-b", 22, &key(KEY_A), Some(&p)),
            Verdict::Unknown
        );
    }
}
