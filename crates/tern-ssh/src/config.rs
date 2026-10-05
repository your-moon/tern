// Adapted from CrabPort crabport-core/src/ssh_import.rs (Apache-2.0): alias enumeration over
// ssh2-config (`get_hosts().skip(1)`, concrete-alias filter, case-insensitive dedupe).
use std::collections::HashSet;
use std::path::PathBuf;

use ssh2_config::{ParseRule, SshConfig};

use crate::error::{Error, Result};
use crate::{ConnectSpec, HostEntry};

const DEFAULT_IDENTITIES: [&str; 3] = ["id_ed25519", "id_ecdsa", "id_rsa"];

/// Effective parameters for one host after applying `~/.ssh/config`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Resolved {
    pub host_name: String,
    pub port: u16,
    pub user: Option<String>,
    pub identity_files: Vec<PathBuf>,
    pub proxy_command: Option<String>,
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::home_dir()
}

pub(crate) fn load_config() -> Option<SshConfig> {
    let path = home_dir()?.join(".ssh").join("config");
    let file = std::fs::File::open(&path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let rules = ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS;
    match SshConfig::default().parse(&mut reader, rules) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(error = %e, "ssh_config_parse_failed");
            None
        }
    }
}

pub(crate) fn resolve(config: Option<&SshConfig>, alias: &str) -> Resolved {
    let Some(config) = config else {
        return Resolved {
            host_name: alias.to_string(),
            port: 22,
            ..Default::default()
        };
    };
    let p = config.query(alias);
    let proxy_command = p
        .unsupported_fields
        .get("proxycommand")
        .map(|args| args.join(" "))
        .filter(|c| !c.trim().is_empty() && !c.trim().eq_ignore_ascii_case("none"));
    Resolved {
        host_name: p.host_name.unwrap_or_else(|| alias.to_string()),
        port: p.port.unwrap_or(22),
        user: p.user,
        identity_files: p.identity_file.unwrap_or_default(),
        proxy_command,
    }
}

fn is_concrete_alias(alias: &str) -> bool {
    !alias.is_empty()
        && !alias.starts_with('!')
        && !alias
            .chars()
            .any(|c| matches!(c, '*' | '?' | '[' | ']' | '\\'))
}

pub(crate) fn hosts_from(config: &SshConfig) -> Vec<HostEntry> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for block in config.get_hosts().iter().skip(1) {
        for clause in &block.pattern {
            let alias = clause.pattern.trim();
            if clause.negated || !is_concrete_alias(alias) {
                continue;
            }
            if !seen.insert(alias.to_ascii_lowercase()) {
                continue;
            }
            let r = resolve(Some(config), alias);
            out.push(HostEntry {
                alias: alias.to_string(),
                host_name: r.host_name,
                port: r.port,
                user: r.user,
                identity_files: r.identity_files,
                proxy_command: r.proxy_command,
            });
        }
    }
    out
}

/// Hosts declared in `~/.ssh/config`, wildcard patterns skipped.
pub fn load_ssh_config_hosts() -> Vec<HostEntry> {
    load_config().map(|c| hosts_from(&c)).unwrap_or_default()
}

pub(crate) fn local_user() -> Option<String> {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
}

/// Default identity files that exist on disk, used when none are configured.
pub(crate) fn default_identity_files() -> Vec<PathBuf> {
    let Some(dir) = home_dir().map(|h| h.join(".ssh")) else {
        return Vec::new();
    };
    DEFAULT_IDENTITIES
        .iter()
        .map(|n| dir.join(n))
        .filter(|p| p.is_file())
        .collect()
}

/// Splits `[user@]host[:port]` (`[v6]:port` for bracketed IPv6).
fn split_target(target: &str) -> Result<(Option<String>, String, Option<u16>)> {
    let t = target.trim();
    if t.is_empty() {
        return Err(Error::EmptyTarget);
    }
    let (user, rest) = match t.rfind('@') {
        Some(i) => {
            let u = &t[..i];
            if u.is_empty() {
                return Err(Error::InvalidTarget(t.to_string()));
            }
            (Some(u.to_string()), &t[i + 1..])
        }
        None => (None, t),
    };
    let parse_port = |p: &str| {
        p.parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| Error::InvalidPort(p.to_string()))
    };
    let (host, port) = if let Some(r) = rest.strip_prefix('[') {
        let end = r
            .find(']')
            .ok_or_else(|| Error::InvalidTarget(t.to_string()))?;
        let after = &r[end + 1..];
        let port = match after {
            "" => None,
            a => Some(parse_port(
                a.strip_prefix(':')
                    .ok_or_else(|| Error::InvalidTarget(t.to_string()))?,
            )?),
        };
        (r[..end].to_string(), port)
    } else if rest.matches(':').count() == 1 {
        let (h, p) = rest.split_once(':').unwrap_or((rest, ""));
        (h.to_string(), Some(parse_port(p)?))
    } else {
        (rest.to_string(), None)
    };
    if host.is_empty() {
        return Err(Error::InvalidTarget(t.to_string()));
    }
    Ok((user, host, port))
}

pub(crate) fn parse_target(target: &str, config: Option<&SshConfig>) -> Result<ConnectSpec> {
    let (user, host, port) = split_target(target)?;
    let r = resolve(config, &host);
    let user = user
        .or(r.user)
        .or_else(local_user)
        .ok_or(Error::NoLocalUser)?;
    Ok(ConnectSpec {
        host: r.host_name,
        port: port.unwrap_or(r.port),
        user,
        identity_files: r.identity_files,
        proxy_command: r.proxy_command,
    })
}

/// Expands `%h`, `%p`, `%r`, `%%` in a ProxyCommand, as OpenSSH does.
pub(crate) fn expand_proxy_command(cmd: &str, host: &str, port: u16, user: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut chars = cmd.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(host),
            Some('p') => out.push_str(&port.to_string()),
            Some('r') => out.push_str(user),
            Some('%') => out.push('%'),
            Some(o) => {
                out.push('%');
                out.push(o);
            }
            None => out.push('%'),
        }
    }
    out
}

impl ConnectSpec {
    /// Builds a connection target from a `~/.ssh/config` host. With no `User`, the local
    /// user name is used, as OpenSSH does.
    ///
    /// # Errors
    ///
    /// [`Error::NoLocalUser`] when the entry has no `User` and the local user name is unknown.
    /// There is deliberately no fallback such as `root`.
    pub fn from_host_entry(e: &HostEntry) -> Result<Self> {
        Ok(ConnectSpec {
            host: e.host_name.clone(),
            port: e.port,
            user: e
                .user
                .clone()
                .or_else(local_user)
                .ok_or(Error::NoLocalUser)?,
            identity_files: e.identity_files.clone(),
            proxy_command: e.proxy_command.clone(),
        })
    }

    /// Parses `[user@]host[:port]`; `host` may be an alias from `~/.ssh/config`
    /// (HostName, Port, User, IdentityFile are taken from it; explicit user/port win).
    ///
    /// # Errors
    ///
    /// [`Error::EmptyTarget`], [`Error::InvalidTarget`] or [`Error::InvalidPort`] for a malformed
    /// target, and [`Error::NoLocalUser`] when no user is given anywhere and the local user name
    /// is unknown.
    pub fn parse(target: &str) -> Result<Self> {
        parse_target(target, load_config().as_ref())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn cfg(s: &str) -> SshConfig {
        let rules = ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS;
        SshConfig::default()
            .parse(&mut s.as_bytes(), rules)
            .unwrap()
    }

    const CONF: &str = "Host web\n  HostName 10.1.2.3\n  Port 2222\n  User deploy\n  IdentityFile /keys/web\n\n\
                        Host plain\n  User bob\n\nHost *\n  ProxyCommand nc %h %p\n";

    const JUMP: &str =
        "Host inner\n  HostName 10.9.9.9\n  User me\n  ProxyCommand ssh -W %h:%p bastion\n";

    #[test]
    fn alias_proxy_command_survives_host_name_resolution() {
        let s = parse_target("inner", Some(&cfg(JUMP))).unwrap();
        assert_eq!(s.host, "10.9.9.9");
        assert_eq!(s.proxy_command.as_deref(), Some("ssh -W %h:%p bastion"));
    }

    #[test]
    fn host_entry_carries_its_proxy_command_into_the_spec() {
        let entry = hosts_from(&cfg(JUMP)).remove(0);
        let s = ConnectSpec::from_host_entry(&entry).unwrap();
        assert_eq!(s.proxy_command.as_deref(), Some("ssh -W %h:%p bastion"));
    }

    #[test]
    fn full_target_without_config() {
        let s = parse_target("alice@example.com:2200", None).unwrap();
        assert_eq!(
            (s.user.as_str(), s.host.as_str(), s.port),
            ("alice", "example.com", 2200)
        );
    }

    #[test]
    fn default_port_and_user_from_env() {
        let s = parse_target("example.com", None).unwrap();
        assert_eq!((s.host.as_str(), s.port), ("example.com", 22));
    }

    #[test]
    fn alias_resolves_through_config() {
        let c = cfg(CONF);
        let s = parse_target("web", Some(&c)).unwrap();
        assert_eq!(
            (s.user.as_str(), s.host.as_str(), s.port),
            ("deploy", "10.1.2.3", 2222)
        );
        assert_eq!(s.identity_files, vec![PathBuf::from("/keys/web")]);
    }

    #[test]
    fn explicit_user_and_port_override_alias() {
        let c = cfg(CONF);
        let s = parse_target("root@web:2022", Some(&c)).unwrap();
        assert_eq!(
            (s.user.as_str(), s.host.as_str(), s.port),
            ("root", "10.1.2.3", 2022)
        );
    }

    #[test]
    fn alias_without_hostname_keeps_alias() {
        let c = cfg(CONF);
        let s = parse_target("plain", Some(&c)).unwrap();
        assert_eq!(
            (s.user.as_str(), s.host.as_str(), s.port),
            ("bob", "plain", 22)
        );
    }

    #[test]
    fn bracketed_ipv6() {
        let s = parse_target("u@[::1]:2200", None).unwrap();
        assert_eq!((s.host.as_str(), s.port), ("::1", 2200));
        let s = parse_target("u@::1", None).unwrap();
        assert_eq!((s.host.as_str(), s.port), ("::1", 22));
    }

    #[test]
    fn rejects_bad_targets() {
        assert!(matches!(parse_target("  ", None), Err(Error::EmptyTarget)));
        assert!(matches!(
            parse_target("h:abc", None),
            Err(Error::InvalidPort(_))
        ));
        assert!(matches!(
            parse_target("h:0", None),
            Err(Error::InvalidPort(_))
        ));
        assert!(matches!(
            parse_target("h:70000", None),
            Err(Error::InvalidPort(_))
        ));
        assert!(matches!(
            parse_target("@h", None),
            Err(Error::InvalidTarget(_))
        ));
        assert!(matches!(
            parse_target("u@", None),
            Err(Error::InvalidTarget(_))
        ));
    }

    #[test]
    fn hosts_skip_wildcards_and_dedupe() {
        let c = cfg(&format!("{CONF}\nHost WEB\n  Port 1\n"));
        let h = hosts_from(&c);
        let aliases: Vec<_> = h.iter().map(|e| e.alias.as_str()).collect();
        assert_eq!(aliases, ["web", "plain"]);
        assert_eq!(h[0].host_name, "10.1.2.3");
    }

    #[test]
    fn proxy_command_is_picked_up_and_expanded() {
        let c = cfg(CONF);
        let r = resolve(Some(&c), "web");
        let cmd = r.proxy_command.unwrap();
        assert_eq!(expand_proxy_command(&cmd, "h", 22, "u"), "nc h 22");
        assert_eq!(expand_proxy_command("a%%b%x", "h", 1, "u"), "a%b%x");
    }
}
