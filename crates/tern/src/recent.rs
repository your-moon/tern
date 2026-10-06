//! Hosts connected to lately, newest first, in `recent.json`. It is per device (when you
//! last used a host here), so it is not synced.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "recent.json";
/// What is kept on disk.
pub const CAP: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub alias: String,
    /// Seconds since the Unix epoch.
    pub at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    entries: Vec<Entry>,
}

impl Recent {
    /// Missing or unreadable file is an empty history; losing it costs nothing.
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join(FILE_NAME))
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
            .map(|mut r| {
                r.entries.sort_by_key(|e| std::cmp::Reverse(e.at));
                r.entries.truncate(CAP);
                r
            })
            .unwrap_or_default()
    }

    /// Temp file + rename.
    ///
    /// # Errors
    /// When the directory or file cannot be written.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE_NAME);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)
    }

    /// Connecting again moves the alias to the front instead of listing it twice.
    pub fn touch(&mut self, alias: &str, at: u64) {
        self.entries.retain(|e| e.alias != alias);
        self.entries.insert(
            0,
            Entry {
                alias: alias.to_owned(),
                at,
            },
        );
        self.entries.truncate(CAP);
    }

    /// Aliases, newest first.
    pub fn aliases(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.alias.as_str())
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn aliases(r: &Recent) -> Vec<&str> {
        r.aliases().collect()
    }

    #[test]
    fn newest_first_and_a_repeat_moves_to_the_front() {
        let mut r = Recent::default();
        r.touch("web", 1);
        r.touch("db", 2);
        r.touch("cache", 3);
        assert_eq!(aliases(&r), ["cache", "db", "web"]);
        r.touch("web", 4);
        assert_eq!(aliases(&r), ["web", "cache", "db"]);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn the_list_is_capped_and_drops_the_oldest() {
        let mut r = Recent::default();
        for i in 0..(CAP + 5) {
            r.touch(&format!("h{i}"), i as u64);
        }
        assert_eq!(r.len(), CAP);
        assert_eq!(r.aliases().next(), Some("h24"));
        assert_eq!(r.aliases().last(), Some("h5"));
    }

    #[test]
    fn it_round_trips_and_a_bad_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Recent::load(dir.path()).len(), 0);
        let mut r = Recent::default();
        r.touch("a", 10);
        r.touch("b", 20);
        r.save(dir.path()).unwrap();
        assert_eq!(Recent::load(dir.path()), r);
        std::fs::write(dir.path().join(FILE_NAME), "{ nope").unwrap();
        assert_eq!(Recent::load(dir.path()).len(), 0);
    }

    #[test]
    fn a_hand_edited_file_is_sorted_and_capped_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let entries: Vec<String> = (0..30)
            .map(|i| format!(r#"{{"alias":"h{i}","at":{i}}}"#))
            .collect();
        let json = format!(r#"{{"entries":[{}]}}"#, entries.join(","));
        std::fs::write(dir.path().join(FILE_NAME), json).unwrap();
        let r = Recent::load(dir.path());
        assert_eq!(r.len(), CAP);
        assert_eq!(r.aliases().next(), Some("h29"));
    }
}
