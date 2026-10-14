//! The run's one Keychain attempt for the vault passphrase: who may start it, who waits for it,
//! and what happens to the vault it opens. No gpui here, so the rules are tested with a fake
//! store that counts reads; the real Keychain is never reached from a test.

use std::path::Path;

use tern_vault::{SecretString, Vault};

use gpui::App;

use crate::keeper::keychain;

/// What the Keychain answered when asked for the passphrase.
pub(crate) enum Load {
    Found(SecretString),
    /// There is no item.
    Missing,
    /// The store refused or failed, as when the person denies the macOS dialog.
    Refused,
}

/// Where the Keychain passphrase comes from: the platform store, or a fake in tests.
pub(crate) trait Passphrases {
    fn load(&self) -> Load;
}

impl Passphrases for keychain::Passphrase {
    fn load(&self) -> Load {
        keychain::read()
    }
}

/// What one Keychain attempt came to.
pub(crate) struct Read {
    pub(crate) vault: Option<Vault>,
    pub(crate) refused: bool,
}

/// Opens the vault at `path` with the stored passphrase; scrypt runs, so call it off the UI
/// thread. No vault for no item, a refusal or a wrong passphrase.
pub(crate) fn open_with(store: &impl Passphrases, path: &Path) -> Read {
    match store.load() {
        Load::Found(passphrase) => Read {
            vault: Vault::unlock(path, passphrase).ok(),
            refused: false,
        },
        Load::Missing => Read {
            vault: None,
            refused: false,
        },
        Load::Refused => Read {
            vault: None,
            refused: true,
        },
    }
}

/// Keeps a vault the Keychain opened only if nothing else opened one meanwhile: the open vault
/// may hold edits the Keychain's copy lacks. The unused copy is dropped, which wipes it.
pub(crate) fn adopt(open: &mut Option<Vault>, found: Option<Vault>) -> bool {
    match (open.is_none(), found) {
        (true, Some(vault)) => {
            *open = Some(vault);
            true
        }
        _ => false,
    }
}

/// Whether `put` writes the passphrase to the Keychain: not over an item the store just refused,
/// which would raise a second dialog.
pub(crate) fn store_due(keychain: bool, current: bool, refused: bool) -> bool {
    keychain && !current && !refused
}

pub(crate) type Waiter = Box<dyn FnOnce(&mut App)>;

/// What a need for the vault does about the Keychain.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Need {
    /// Ask the Keychain now; the need waits for it.
    Start,
    /// An attempt is running; the need waits for it.
    Wait,
    /// The Keychain is not to be asked.
    Skip,
}

/// The once-per-run Keychain attempt and the needs waiting on it.
pub(crate) struct Gate<W> {
    tried: bool,
    in_flight: bool,
    pub(crate) refused: bool,
    waiting: Vec<W>,
}

impl<W> Default for Gate<W> {
    fn default() -> Self {
        Self {
            tried: false,
            in_flight: false,
            refused: false,
            waiting: Vec::new(),
        }
    }
}

impl<W> Gate<W> {
    /// A need for the locked vault arrives; `waiter` is kept unless the answer is `Skip`.
    pub(crate) fn need(&mut self, enabled: bool, locked: bool, waiter: W) -> Need {
        if self.in_flight {
            self.waiting.push(waiter);
            Need::Wait
        } else if enabled && !self.tried && locked {
            self.tried = true;
            self.in_flight = true;
            self.waiting.push(waiter);
            Need::Start
        } else {
            Need::Skip
        }
    }

    /// The attempt resolved; hands back everything that waited, in order.
    pub(crate) fn finish(&mut self, refused: bool) -> Vec<W> {
        self.in_flight = false;
        self.refused |= refused;
        std::mem::take(&mut self.waiting)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use tern_vault::Key;

    use super::*;

    /// Counts reads; a fake for the Keychain so no test reaches the real one.
    struct Counting {
        answer: fn() -> Load,
        loads: std::cell::Cell<u32>,
    }

    impl Counting {
        fn new(answer: fn() -> Load) -> Self {
            Self {
                answer,
                loads: std::cell::Cell::new(0),
            }
        }
    }

    impl Passphrases for Counting {
        fn load(&self) -> Load {
            self.loads.set(self.loads.get() + 1);
            (self.answer)()
        }
    }

    fn found(p: &str) -> Load {
        Load::Found(SecretString::from(p.to_owned()))
    }
    fn right() -> Load {
        found("right")
    }
    fn stale() -> Load {
        found("changed elsewhere")
    }
    fn missing() -> Load {
        Load::Missing
    }
    fn refused() -> Load {
        Load::Refused
    }

    /// One need as `Keeper::keychain_then` makes it: the gate decides, and only `Start` reads.
    /// Returns what the need did and, when it started the read, what the read came to.
    fn need(gate: &mut Gate<u32>, store: &Counting, path: &Path, id: u32) -> (Need, Option<Read>) {
        let n = gate.need(true, true, id);
        let read = (n == Need::Start).then(|| open_with(store, path));
        (n, read)
    }

    fn vault_file(pass: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.age");
        Vault::new(SecretString::from(pass.to_owned()))
            .save(&path)
            .unwrap();
        (dir, path)
    }

    #[test]
    fn the_first_need_reads_once_and_a_failed_attempt_is_not_repeated() {
        let (_dir, path) = vault_file("right");
        let store = Counting::new(stale);
        let mut gate = Gate::default();
        let (n, read) = need(&mut gate, &store, &path, 1);
        assert_eq!(n, Need::Start);
        assert!(read.unwrap().vault.is_none());
        gate.finish(false);
        for id in 2..5 {
            assert_eq!(need(&mut gate, &store, &path, id).0, Need::Skip);
        }
        assert_eq!(store.loads.get(), 1, "no second dialog this run");
    }

    #[test]
    fn a_refusal_or_missing_item_counts_as_the_attempt() {
        let (_dir, path) = vault_file("right");
        for answer in [missing, refused] {
            let store = Counting::new(answer);
            let mut gate = Gate::default();
            assert_eq!(need(&mut gate, &store, &path, 1).0, Need::Start);
            gate.finish(false);
            assert_eq!(need(&mut gate, &store, &path, 2).0, Need::Skip);
            assert_eq!(store.loads.get(), 1);
        }
    }

    #[test]
    fn a_need_during_the_attempt_waits_for_it_and_is_woken_in_order() {
        let (_dir, path) = vault_file("right");
        let store = Counting::new(right);
        let mut gate = Gate::default();
        let (first, read) = need(&mut gate, &store, &path, 1);
        assert_eq!(first, Need::Start);
        // Still in flight: a second session, the form and the page join instead of prompting.
        assert_eq!(need(&mut gate, &store, &path, 2).0, Need::Wait);
        assert_eq!(need(&mut gate, &store, &path, 3).0, Need::Wait);
        assert!(read.unwrap().vault.is_some());
        assert_eq!(gate.finish(false), vec![1, 2, 3]);
        assert_eq!(store.loads.get(), 1);
        assert_eq!(need(&mut gate, &store, &path, 4).0, Need::Skip);
    }

    #[test]
    fn the_keychain_is_not_asked_when_off_or_when_the_vault_is_already_open() {
        let mut gate = Gate::default();
        assert_eq!(gate.need(false, true, 1), Need::Skip);
        assert_eq!(gate.need(true, false, 2), Need::Skip);
        assert!(
            !gate.tried,
            "a need that did not ask keeps the run's one try"
        );
        assert!(gate.waiting.is_empty());
    }

    #[test]
    fn the_stored_passphrase_opens_the_vault_without_a_prompt() {
        let (_dir, path) = vault_file("right");
        assert!(open_with(&Counting::new(right), &path).vault.is_some());
    }

    #[test]
    fn a_stale_passphrase_leaves_the_vault_locked() {
        let (_dir, path) = vault_file("right");
        let read = open_with(&Counting::new(stale), &path);
        assert!(read.vault.is_none() && !read.refused);
        assert!(
            open_with(&Counting::new(right), &path.with_file_name("none.age"))
                .vault
                .is_none()
        );
    }

    #[test]
    fn a_vault_opened_meanwhile_is_not_replaced_by_the_keychains_copy() {
        let (_dir, path) = vault_file("right");
        let key = Key::SshKey { name: "k".into() };
        let mut open = Some(Vault::unlock(&path, SecretString::from("right".to_owned())).unwrap());
        open.as_mut()
            .unwrap()
            .set(key.clone(), SecretString::from("edited".to_owned()));
        let stale_copy = open_with(&Counting::new(right), &path).vault;
        assert!(!adopt(&mut open, stale_copy));
        assert!(open.unwrap().get(&key).is_some(), "the edit survives");
        let mut locked = None;
        let fresh = open_with(&Counting::new(right), &path).vault;
        assert!(adopt(&mut locked, fresh) && locked.is_some());
    }

    #[test]
    fn a_refused_keychain_is_not_written_over_for_the_rest_of_the_run() {
        let mut gate: Gate<u32> = Gate::default();
        assert!(store_due(true, false, gate.refused));
        gate.need(true, true, 1);
        gate.finish(false);
        assert!(
            store_due(true, false, gate.refused),
            "no item: store as before"
        );
        let mut gate: Gate<u32> = Gate::default();
        gate.need(true, true, 1);
        gate.finish(true);
        assert!(!store_due(true, false, gate.refused));
        assert!(!store_due(false, false, false));
        assert!(!store_due(true, true, false));
        let (_dir, path) = vault_file("right");
        assert!(open_with(&Counting::new(refused), &path).refused);
    }
}
