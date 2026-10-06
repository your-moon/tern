//! Sync through a private GitHub gist described `tern-sync`, as Tabby syncs its settings.
//!
//! What is uploaded: the vault file as it is (already age-encrypted), and the other files
//! (hosts, settings, keymap) sealed with the vault passphrase by the caller, plus a small
//! plain `tern-meta.json` holding the bundle hash and time. Nothing readable leaves the Mac.
//!
//! Deciding what to do needs no network: compare the local and remote bundle hashes with the
//! hash both sides had at the last sync ([`decide`]).

mod github;

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

pub use github::{Gist, GithubError, RemoteBundle, TokenSource, forget_token, save_token, token};

/// The files that travel, by name, as raw bytes.
pub type Files = BTreeMap<String, Vec<u8>>;

/// A stable fingerprint of a bundle: SHA-256 over name, length and bytes of each file in
/// name order, so renaming, reordering or moving bytes between files all change it.
pub fn hash(files: &Files) -> String {
    let mut h = Sha256::new();
    for (name, bytes) in files {
        h.update(name.as_bytes());
        h.update([0]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    UpToDate,
    Push,
    Pull,
    /// Both sides changed since the last sync: the user picks which one wins.
    Conflict,
}

/// What a sync should do. `last` is the hash both sides agreed on at the previous sync,
/// `None` on a Mac that never synced.
pub fn decide(local: &str, remote: Option<&str>, last: Option<&str>) -> Plan {
    let Some(remote) = remote else {
        return Plan::Push;
    };
    if local == remote {
        return Plan::UpToDate;
    }
    match last {
        // First sync on this Mac and the two differ: neither side can be called stale.
        None => Plan::Conflict,
        Some(last) => match (local != last, remote != last) {
            (true, true) => Plan::Conflict,
            (true, false) => Plan::Push,
            (false, true) => Plan::Pull,
            (false, false) => Plan::UpToDate,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(pairs: &[(&str, &str)]) -> Files {
        pairs
            .iter()
            .map(|(n, b)| ((*n).to_owned(), b.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn the_decision_table() {
        assert_eq!(decide("a", None, None), Plan::Push);
        assert_eq!(decide("a", Some("a"), None), Plan::UpToDate);
        assert_eq!(decide("a", Some("b"), None), Plan::Conflict);
        assert_eq!(decide("new", Some("old"), Some("old")), Plan::Push);
        assert_eq!(decide("old", Some("new"), Some("old")), Plan::Pull);
        assert_eq!(decide("x", Some("y"), Some("old")), Plan::Conflict);
        assert_eq!(decide("same", Some("same"), Some("old")), Plan::UpToDate);
    }

    #[test]
    fn hash_sees_content_names_and_boundaries() {
        let base = hash(&files(&[("a", "12"), ("b", "3")]));
        assert_eq!(
            base,
            hash(&files(&[("b", "3"), ("a", "12")])),
            "map order is canonical"
        );
        assert_ne!(base, hash(&files(&[("a", "1"), ("b", "23")])), "moved byte");
        assert_ne!(
            base,
            hash(&files(&[("a", "12"), ("c", "3")])),
            "renamed file"
        );
        assert_ne!(
            base,
            hash(&files(&[("a", "12"), ("b", "4")])),
            "changed byte"
        );
        assert_eq!(base.len(), 64);
    }
}
