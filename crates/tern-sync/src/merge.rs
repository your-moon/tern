//! Merging two diverged bundles item by item instead of asking about the whole thing.
//!
//! An item is one synced file (hosts, settings, keymap, vault). [`decide_item`] compares each
//! side's hash of the item with the hash both sides had at the last sync; only an item that
//! changed on both sides is a real conflict. `hosts.json` is merged further, per connection
//! name, with [`merge_hosts`] so two Macs that each added a host both keep both.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::Files;

/// The hash recorded for an item whose file does not exist.
pub const ABSENT: &str = "-";

/// A hash per item: SHA-256 of the file's bytes, [`ABSENT`] when the file is missing.
pub fn item_hashes(files: &Files, items: &[&str]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|name| {
            let hash = files.get(*name).map_or_else(
                || ABSENT.to_owned(),
                |bytes| {
                    Sha256::digest(bytes)
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect()
                },
            );
            ((*name).to_owned(), hash)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemPlan {
    Same,
    /// Only this Mac changed it: the remote takes this Mac's copy.
    KeepLocal,
    /// Only the remote changed it: this Mac takes the remote's copy.
    TakeRemote,
    /// Both changed it (or there is no record of the last sync): needs a merge or a choice.
    Both,
}

/// The per-item version of [`crate::decide`]. `last` is the item's hash at the last sync,
/// `None` when unknown.
pub fn decide_item(local: &str, remote: &str, last: Option<&str>) -> ItemPlan {
    if local == remote {
        return ItemPlan::Same;
    }
    match last {
        None => ItemPlan::Both,
        Some(last) => match (local != last, remote != last) {
            (true, true) => ItemPlan::Both,
            (true, false) => ItemPlan::KeepLocal,
            (false, true) => ItemPlan::TakeRemote,
            (false, false) => ItemPlan::Same,
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum HostsMerge {
    /// The merged `hosts.json` text.
    Merged(String),
    /// Connections edited differently on both sides, or edited on one side and removed on
    /// the other: by name.
    Conflict(Vec<String>),
}

fn connections(text: &str) -> Result<Vec<(String, Value)>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("hosts.json: {e}"))?;
    let list = v
        .get("connections")
        .and_then(Value::as_array)
        .ok_or("hosts.json: no connections list")?;
    list.iter()
        .map(|c| {
            let name = c
                .get("name")
                .and_then(Value::as_str)
                .ok_or("hosts.json: a connection has no name")?;
            Ok((name.to_owned(), c.clone()))
        })
        .collect()
}

/// Three-way merge of `hosts.json` by connection name. `base` is the text at the last sync.
///
/// Per name: unchanged on one side takes the other side's state (an edit, an addition or a
/// removal); changed the same way on both is that; changed differently on both is a conflict.
/// Order is this Mac's, then the connections only the remote has.
///
/// # Errors
/// One of the three texts is not a `hosts.json`.
pub fn merge_hosts(base: &str, local: &str, remote: &str) -> Result<HostsMerge, String> {
    let (base, local, remote) = (
        connections(base)?,
        connections(local)?,
        connections(remote)?,
    );
    let find = |list: &[(String, Value)], name: &str| {
        list.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())
    };
    let mut order: Vec<&str> = Vec::new();
    for (n, _) in local.iter().chain(&remote) {
        if !order.contains(&n.as_str()) {
            order.push(n);
        }
    }
    let (mut out, mut conflicts) = (Vec::new(), Vec::new());
    for name in order {
        let (b, l, r) = (find(&base, name), find(&local, name), find(&remote, name));
        let pick = if l == r || r == b {
            l
        } else if l == b {
            r
        } else {
            conflicts.push(name.to_owned());
            continue;
        };
        out.extend(pick);
    }
    if !conflicts.is_empty() {
        return Ok(HostsMerge::Conflict(conflicts));
    }
    let mut file = Map::new();
    file.insert("version".into(), json!(1));
    file.insert("connections".into(), Value::Array(out));
    serde_json::to_string_pretty(&Value::Object(file))
        .map(HostsMerge::Merged)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn conn(name: &str, host: &str) -> Value {
        json!({"name": name, "host": host, "port": 22, "user": "root"})
    }

    fn file(list: &[Value]) -> String {
        serde_json::to_string_pretty(&json!({"version": 1, "connections": list})).unwrap()
    }

    fn merged(base: &[Value], local: &[Value], remote: &[Value]) -> HostsMerge {
        merge_hosts(&file(base), &file(local), &file(remote)).unwrap()
    }

    fn names(m: HostsMerge) -> Vec<String> {
        let HostsMerge::Merged(text) = m else {
            return vec![format!("not a clean merge: {m:?}")];
        };
        connections(&text)
            .unwrap()
            .into_iter()
            .map(|(n, v)| format!("{n}={}", v["host"].as_str().unwrap()))
            .collect()
    }

    #[test]
    fn item_table() {
        use ItemPlan::*;
        assert_eq!(decide_item("a", "a", Some("old")), Same);
        assert_eq!(decide_item("new", "old", Some("old")), KeepLocal);
        assert_eq!(decide_item("old", "new", Some("old")), TakeRemote);
        assert_eq!(decide_item("x", "y", Some("old")), Both);
        assert_eq!(decide_item("x", "y", None), Both, "no record: ask");
        assert_eq!(decide_item("x", "x", None), Same);
        // A file created on one side only: base is absent.
        assert_eq!(decide_item("k", ABSENT, Some(ABSENT)), KeepLocal);
        assert_eq!(decide_item(ABSENT, "k", Some(ABSENT)), TakeRemote);
    }

    #[test]
    fn item_hashes_tell_files_apart() {
        let mut f = Files::new();
        f.insert("hosts.json".into(), b"{}".to_vec());
        f.insert("keymap.json".into(), b"[]".to_vec());
        let h = item_hashes(&f, &["hosts.json", "settings.json", "keymap.json"]);
        assert_eq!(h["settings.json"], ABSENT);
        assert_ne!(h["hosts.json"], h["keymap.json"]);
        assert_eq!(h["hosts.json"].len(), 64);
    }

    #[test]
    fn two_macs_add_different_hosts_keep_both() {
        let base = [conn("web", "1.1.1.1")];
        let local = [conn("web", "1.1.1.1"), conn("db", "2.2.2.2")];
        let remote = [conn("web", "1.1.1.1"), conn("cache", "3.3.3.3")];
        assert_eq!(
            names(merged(&base, &local, &remote)),
            ["web=1.1.1.1", "db=2.2.2.2", "cache=3.3.3.3"]
        );
    }

    #[test]
    fn removed_on_one_side_and_untouched_on_the_other_is_removed() {
        let base = [conn("web", "1.1.1.1"), conn("db", "2.2.2.2")];
        let removed = [conn("web", "1.1.1.1")];
        assert_eq!(names(merged(&base, &base, &removed)), ["web=1.1.1.1"]);
        // And the other way round: this Mac removed it.
        assert_eq!(names(merged(&base, &removed, &base)), ["web=1.1.1.1"]);
    }

    #[test]
    fn removed_on_one_side_and_edited_on_the_other_is_a_conflict() {
        let base = [conn("web", "1.1.1.1"), conn("db", "2.2.2.2")];
        let local = [conn("web", "1.1.1.1")];
        let remote = [conn("web", "1.1.1.1"), conn("db", "9.9.9.9")];
        assert_eq!(
            merged(&base, &local, &remote),
            HostsMerge::Conflict(vec!["db".into()])
        );
        assert_eq!(
            merged(&base, &remote, &local),
            HostsMerge::Conflict(vec!["db".into()])
        );
    }

    #[test]
    fn both_edit_different_hosts_takes_each_edit() {
        let base = [conn("web", "1.1.1.1"), conn("db", "2.2.2.2")];
        let local = [conn("web", "10.0.0.1"), conn("db", "2.2.2.2")];
        let remote = [conn("web", "1.1.1.1"), conn("db", "20.0.0.2")];
        assert_eq!(
            names(merged(&base, &local, &remote)),
            ["web=10.0.0.1", "db=20.0.0.2"]
        );
    }

    #[test]
    fn both_edit_the_same_host_differently_is_a_conflict_but_alike_is_not() {
        let base = [conn("web", "1.1.1.1"), conn("db", "2.2.2.2")];
        let local = [conn("web", "10.0.0.1"), conn("db", "5.5.5.5")];
        let remote = [conn("web", "11.0.0.1"), conn("db", "5.5.5.5")];
        assert_eq!(
            merged(&base, &local, &remote),
            HostsMerge::Conflict(vec!["web".into()])
        );
    }

    #[test]
    fn same_name_added_on_both_sides_differently_is_a_conflict() {
        let base: [Value; 0] = [];
        assert_eq!(
            merged(&base, &[conn("x", "1.1.1.1")], &[conn("x", "2.2.2.2")]),
            HostsMerge::Conflict(vec!["x".into()])
        );
        assert_eq!(
            names(merged(
                &base,
                &[conn("x", "1.1.1.1")],
                &[conn("x", "1.1.1.1")]
            )),
            ["x=1.1.1.1"]
        );
    }

    #[test]
    fn unknown_fields_survive_and_a_missing_file_is_empty() {
        let mut c = conn("web", "1.1.1.1");
        c["identityFile"] = json!("~/.ssh/id_ed25519");
        let out = merge_hosts("", &file(&[c.clone()]), "").unwrap();
        let HostsMerge::Merged(text) = &out else {
            unreachable!("{out:?}")
        };
        assert_eq!(connections(text).unwrap()[0].1, c);
    }
}
