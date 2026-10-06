//! Connections made in tern, kept in `hosts.json` next to the settings. tern never writes
//! `~/.ssh/config`: it is often generated (assh, Nix, chezmoi) and an edit there would be lost.
//! A connection with the same name as a `~/.ssh/config` host takes its place in the list.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tern_ssh::HostEntry;

const FILE_NAME: &str = "hosts.json";
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// A private key path; `None` tries ssh-agent and the default keys, as `ssh` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    /// The folder it is listed under in the sidebar; `None` lists it with the ungrouped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Free labels the sidebar search also matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Copied from `~/.ssh/config` on import; the form does not edit it, so an edit keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_command: Option<String>,
    /// Name of a private key kept in the vault, offered at login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_key: Option<String>,
}

impl Connection {
    /// The connection as a host entry, so it connects and lists like a `~/.ssh/config` host.
    pub fn entry(&self) -> HostEntry {
        HostEntry {
            alias: self.name.clone(),
            host_name: self.host.clone(),
            port: self.port,
            user: Some(self.user.clone()),
            identity_files: self.identity_file.iter().map(|p| expand_home(p)).collect(),
            proxy_command: self.proxy_command.clone(),
        }
    }

    /// A draft copied from a `~/.ssh/config` host, for "Duplicate to edit".
    pub fn from_entry(entry: &HostEntry) -> Self {
        Self {
            name: entry.alias.clone(),
            host: entry.host_name.clone(),
            port: entry.port,
            user: entry.user.clone().unwrap_or_default(),
            identity_file: entry
                .identity_files
                .first()
                .map(|p| p.display().to_string()),
            group: None,
            tags: Vec::new(),
            proxy_command: entry.proxy_command.clone(),
            vault_key: None,
        }
    }
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    connections: Vec<Connection>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("hosts.json could not be read ({0}); fix or move it, tern will not overwrite it")]
    Unreadable(String),
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Reads `hosts.json`. A missing file is an empty list; a file that does not parse is an
/// error, so the caller can refuse to save over it rather than lose the user's connections.
///
/// # Errors
/// [`StoreError::Unreadable`] when the file exists but is not valid; [`StoreError::Io`] when it
/// cannot be read.
pub fn load(dir: &Path) -> Result<Vec<Connection>, StoreError> {
    let text = match std::fs::read_to_string(dir.join(FILE_NAME)) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let file: File =
        serde_json::from_str(&text).map_err(|e| StoreError::Unreadable(e.to_string()))?;
    if file.version != VERSION {
        return Err(StoreError::Unreadable(format!("version {}", file.version)));
    }
    Ok(file.connections)
}

/// Writes `hosts.json` through a temp file and rename.
///
/// # Errors
/// [`StoreError::Io`] when the directory or file cannot be written.
pub fn save(dir: &Path, connections: &[Connection]) -> Result<(), StoreError> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&File {
        version: VERSION,
        connections: connections.to_vec(),
    })
    .map_err(io::Error::other)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// The form's text, as typed.
#[derive(Debug, Default, Clone)]
pub struct Draft {
    pub name: String,
    pub host: String,
    pub port: String,
    pub user: String,
    pub identity_file: String,
    pub group: String,
    /// Comma separated, as typed.
    pub tags: String,
    pub vault_key: String,
}

/// `"prod, web ,,prod"` → `["prod", "web"]`: trimmed, no blanks, no repeats, first spelling kept.
pub fn parse_tags(text: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            tags.push(tag.to_owned());
        }
    }
    tags
}

/// Checks a draft. `others` are the names of the other tern connections (not the one being
/// edited); reusing a `~/.ssh/config` alias is allowed and replaces that host.
///
/// # Errors
/// A message for the first field that is wrong, worded for the form.
pub fn validate(draft: &Draft, others: &[&str]) -> Result<Connection, String> {
    let name = draft.name.trim();
    let host = draft.host.trim();
    let user = draft.user.trim();
    if name.is_empty() {
        return Err("Name is required.".into());
    }
    if others.contains(&name) {
        return Err(format!("A connection named \"{name}\" already exists."));
    }
    if host.is_empty() {
        return Err("Host is required.".into());
    }
    if host.contains(char::is_whitespace) {
        return Err("Host cannot contain spaces.".into());
    }
    let port = match draft.port.trim() {
        "" => 22,
        p => match p.parse::<u16>() {
            Ok(n) if n > 0 => n,
            _ => return Err("Port must be a number from 1 to 65535.".into()),
        },
    };
    if user.is_empty() {
        return Err("User is required.".into());
    }
    let identity_file = Some(draft.identity_file.trim())
        .filter(|p| !p.is_empty())
        .map(str::to_owned);
    let vault_key = Some(draft.vault_key.trim())
        .filter(|k| !k.is_empty())
        .map(str::to_owned);
    Ok(Connection {
        name: name.to_owned(),
        host: host.to_owned(),
        port,
        user: user.to_owned(),
        identity_file,
        group: Some(draft.group.trim())
            .filter(|g| !g.is_empty())
            .map(str::to_owned),
        tags: parse_tags(&draft.tags),
        proxy_command: None,
        vault_key,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn draft(name: &str, host: &str, port: &str, user: &str) -> Draft {
        Draft {
            name: name.into(),
            host: host.into(),
            port: port.into(),
            user: user.into(),
            identity_file: String::new(),
            group: String::new(),
            tags: String::new(),
            vault_key: String::new(),
        }
    }

    #[test]
    fn a_complete_draft_becomes_a_connection_with_port_22_by_default() {
        let c = validate(&draft(" web ", "10.0.0.5", "", "deploy"), &[]).unwrap();
        assert_eq!(c.name, "web");
        assert_eq!(c.port, 22);
        assert_eq!(c.identity_file, None);
    }

    #[test]
    fn a_vault_key_name_is_trimmed_kept_and_survives_the_file() {
        let mut d = draft("web", "10.0.0.5", "", "deploy");
        d.vault_key = "  laptop ".into();
        let c = validate(&d, &[]).unwrap();
        assert_eq!(c.vault_key.as_deref(), Some("laptop"));
        let dir = std::env::temp_dir().join(format!("tern-conn-vk-{}", std::process::id()));
        save(&dir, std::slice::from_ref(&c)).unwrap();
        assert_eq!(load(&dir).unwrap(), vec![c]);
        let none = validate(&draft("web", "h", "", "u"), &[]).unwrap();
        assert_eq!(none.vault_key, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_bad_field_is_named() {
        assert!(
            validate(&draft("", "h", "", "u"), &[])
                .unwrap_err()
                .contains("Name")
        );
        assert!(
            validate(&draft("a", "", "", "u"), &[])
                .unwrap_err()
                .contains("Host")
        );
        assert!(
            validate(&draft("a", "h h", "", "u"), &[])
                .unwrap_err()
                .contains("spaces")
        );
        assert!(
            validate(&draft("a", "h", "0", "u"), &[])
                .unwrap_err()
                .contains("Port")
        );
        assert!(
            validate(&draft("a", "h", "70000", "u"), &[])
                .unwrap_err()
                .contains("Port")
        );
        assert!(
            validate(&draft("a", "h", "x", "u"), &[])
                .unwrap_err()
                .contains("Port")
        );
        assert!(
            validate(&draft("a", "h", "", " "), &[])
                .unwrap_err()
                .contains("User")
        );
    }

    #[test]
    fn names_are_unique_among_tern_connections() {
        let err = validate(&draft("web", "h", "", "u"), &["db", "web"]).unwrap_err();
        assert!(err.contains("already exists"));
        assert!(validate(&draft("web", "h", "", "u"), &["db"]).is_ok());
    }

    #[test]
    fn missing_file_is_empty_and_save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
        let list = vec![validate(&draft("web", "10.0.0.5", "2222", "deploy"), &[]).unwrap()];
        save(dir.path(), &list).unwrap();
        assert_eq!(load(dir.path()).unwrap(), list);
    }

    #[test]
    fn a_broken_file_is_an_error_not_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "{ nope").unwrap();
        assert!(matches!(load(dir.path()), Err(StoreError::Unreadable(_))));
    }

    #[test]
    fn duplicating_an_ssh_config_host_keeps_its_fields() {
        let entry = HostEntry {
            alias: "grape".into(),
            host_name: "203.0.113.40".into(),
            port: 22,
            user: Some("root".into()),
            identity_files: vec![PathBuf::from("/k/id")],
            proxy_command: None,
        };
        let c = Connection::from_entry(&entry);
        assert_eq!((c.name.as_str(), c.user.as_str()), ("grape", "root"));
        assert_eq!(c.identity_file.as_deref(), Some("/k/id"));
        assert_eq!(c.entry().host_name, "203.0.113.40");
        assert_eq!(c.entry().proxy_command, None);
    }

    /// A `hosts.json` written before groups and tags existed must load unchanged, and saving
    /// it back must not invent the new fields.
    #[test]
    fn an_old_hosts_file_without_group_or_tags_loads_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let old = r#"{
  "version": 1,
  "connections": [
    { "name": "web", "host": "10.0.0.5", "port": 2222, "user": "deploy", "identityFile": "~/k" },
    { "name": "db", "host": "10.0.0.6", "port": 22, "user": "root" }
  ]
}"#;
        std::fs::write(dir.path().join(FILE_NAME), old).unwrap();
        let list = load(dir.path()).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].port, 2222);
        assert_eq!(list[0].identity_file.as_deref(), Some("~/k"));
        assert!(list.iter().all(|c| c.group.is_none() && c.tags.is_empty()));
        save(dir.path(), &list).unwrap();
        let text = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
        assert!(!text.contains("group") && !text.contains("tags"), "{text}");
        assert_eq!(load(dir.path()).unwrap(), list);
    }

    #[test]
    fn group_and_tags_survive_a_round_trip_and_are_cleaned() {
        let mut d = draft("web", "h", "", "u");
        d.group = "  Production ".into();
        d.tags = "eu, web ,,EU,db".into();
        let c = validate(&d, &[]).unwrap();
        assert_eq!(c.group.as_deref(), Some("Production"));
        assert_eq!(c.tags, vec!["eu", "web", "db"]);
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), std::slice::from_ref(&c)).unwrap();
        assert_eq!(load(dir.path()).unwrap(), vec![c]);
        let blank = validate(&draft("web", "h", "", "u"), &[]).unwrap();
        assert_eq!((blank.group, blank.tags), (None, Vec::new()));
    }
}
