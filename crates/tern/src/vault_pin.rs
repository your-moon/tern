//! Unlocking the vault with a PIN: the sealed passphrase from the Keychain, the wrong-PIN
//! counter and what happens when it runs out. The sealing itself is `tern_vault::pin`.
//!
//! The counter lives in `vault-pin.json` in the settings directory, which is not one of the
//! synced files, so quitting the app does not reset it and another Mac's count is not ours. It is
//! written before a PIN is checked, so killing the app mid-check does not buy a free guess.

use std::path::Path;

use tern_vault::{SecretString, Vault, VaultError, pin};

use crate::keychain;

/// This many wrong PINs in a row delete the sealed passphrase.
pub const MAX_WRONG: u32 = 5;
const FILE: &str = "vault-pin.json";

/// What one PIN try came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Attempt {
    Right,
    /// Wrong; this many tries remain.
    Wrong {
        left: u32,
    },
    /// Wrong, and the last try: the sealed passphrase is to be deleted.
    Wipe,
}

/// The verdict for a try that brought the stored count to `count` (this try included).
pub fn verdict(count: u32, correct: bool) -> Attempt {
    if correct {
        Attempt::Right
    } else if count >= MAX_WRONG {
        Attempt::Wipe
    } else {
        Attempt::Wrong {
            left: MAX_WRONG - count,
        }
    }
}

fn count(dir: &Path) -> u32 {
    std::fs::read(dir.join(FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("wrong")?.as_u64())
        .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

fn store(dir: &Path, n: u32) {
    let json = serde_json::json!({ "wrong": n }).to_string();
    let tmp = dir.join(format!("{FILE}.tmp"));
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&tmp, json))
        .and_then(|()| std::fs::rename(&tmp, dir.join(FILE)));
    if let Err(e) = written {
        tracing::warn!(error = %e, "vault_pin_counter_unwritten");
    }
}

/// Starts counting from zero: a new PIN, or the item is gone.
pub fn reset(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(FILE));
}

/// The result of checking a PIN against a sealed blob.
#[derive(Debug)]
pub enum Check {
    Passphrase(SecretString),
    Wrong {
        left: u32,
    },
    /// The last wrong PIN: `wipe` ran and the counter was reset.
    Wiped,
    /// The blob is not what `pin::wrap` makes.
    Broken(String),
}

/// Checks `entered` against `blob`, keeping the count in `dir`. `wipe` runs on the wrong PIN
/// that uses up the last try.
pub fn check(dir: &Path, blob: &[u8], entered: &str, wipe: impl FnOnce()) -> Check {
    let now = count(dir).saturating_add(1);
    store(dir, now);
    match pin::unwrap(blob, entered) {
        Ok(passphrase) => {
            reset(dir);
            Check::Passphrase(passphrase)
        }
        Err(VaultError::WrongPassphrase) => match verdict(now, false) {
            Attempt::Wipe => {
                wipe();
                reset(dir);
                Check::Wiped
            }
            Attempt::Wrong { left } => Check::Wrong { left },
            Attempt::Right => Check::Broken("PIN check".into()),
        },
        Err(e) => Check::Broken(e.to_string()),
    }
}

/// Where the sealed passphrase is kept: the login Keychain, or memory in tests.
pub trait Store {
    fn load(&self) -> Option<Vec<u8>>;
    fn save(&self, blob: &[u8]) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

/// The login Keychain item `tern vault pin`.
pub struct KeychainStore;

impl Store for KeychainStore {
    fn load(&self) -> Option<Vec<u8>> {
        keychain::load_pin_blob()
    }
    fn save(&self, blob: &[u8]) -> Result<(), String> {
        keychain::store_pin_blob(blob)
    }
    fn delete(&self) -> Result<(), String> {
        keychain::delete_pin_blob()
    }
}

/// What a PIN unlock did.
#[derive(Debug)]
pub enum Outcome {
    Opened(Vault),
    Wrong {
        left: u32,
    },
    Wiped,
    /// No sealed passphrase any more, or it no longer opens the vault: PIN unlock is off.
    Gone,
    Failed(String),
}

/// Unlocks the vault at `vault` with `entered`. Runs scrypt twice, so call it off the UI thread.
pub fn unlock(dir: &Path, vault: &Path, entered: &str, store: &impl Store) -> Outcome {
    let Some(blob) = store.load() else {
        return Outcome::Gone;
    };
    let wipe = || {
        if let Err(e) = store.delete() {
            tracing::warn!(error = %e, "vault_pin_wipe_failed");
        }
    };
    match check(dir, &blob, entered, wipe) {
        Check::Passphrase(passphrase) => match Vault::unlock(vault, passphrase) {
            Ok(v) => Outcome::Opened(v),
            // The passphrase changed behind our back (a synced vault): the item is stale.
            Err(VaultError::WrongPassphrase) => {
                wipe();
                reset(dir);
                Outcome::Gone
            }
            Err(e) => Outcome::Failed(format!("Vault unavailable: {e}")),
        },
        Check::Wrong { left } => Outcome::Wrong { left },
        Check::Wiped => Outcome::Wiped,
        Check::Broken(e) => {
            wipe();
            reset(dir);
            tracing::warn!(error = %e, "vault_pin_blob_unreadable");
            Outcome::Gone
        }
    }
}

/// Seals `passphrase` to `entered` and starts the count over.
///
/// # Errors
/// A message for the person when the PIN is not acceptable or the store refuses.
pub fn set(
    dir: &Path,
    passphrase: &SecretString,
    entered: &str,
    store: &impl Store,
) -> Result<(), String> {
    if let Some(problem) = pin::problem(entered) {
        return Err(problem.into());
    }
    let blob = pin::wrap(passphrase, entered).map_err(|e| e.to_string())?;
    store.save(&blob)?;
    reset(dir);
    Ok(())
}

/// Deletes the sealed passphrase and the count. Used when the PIN is turned off, and when the
/// vault passphrase changes: the sealed one is then stale, and the PIN must be set again.
///
/// # Errors
/// A message when the store refuses.
pub fn clear(dir: &Path, store: &impl Store) -> Result<(), String> {
    reset(dir);
    store.delete()
}

/// What to tell the person after `outcome`; `None` when the vault opened.
pub fn message(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Opened(_) => None,
        Outcome::Wrong { left } => Some(format!(
            "Wrong PIN, {left} {} left.",
            if *left == 1 { "try" } else { "tries" }
        )),
        Outcome::Wiped => Some(WIPED.into()),
        Outcome::Gone => Some("PIN unlock is off; use your passphrase.".into()),
        Outcome::Failed(e) => Some(e.clone()),
    }
}

/// Shown when the fifth wrong PIN deleted the sealed passphrase.
pub const WIPED: &str = "Too many wrong PINs — PIN unlock is off; use your passphrase";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use tern_vault::ExposeSecret;

    fn blob(pass: &str, pin_text: &str) -> Vec<u8> {
        pin::wrap(&SecretString::from(pass.to_owned()), pin_text).unwrap()
    }

    #[test]
    fn the_fifth_wrong_pin_in_a_row_wipes_and_the_first_four_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let sealed = blob("vault pass", "4821");
        let wiped = Cell::new(0);
        for expect_left in [4, 3, 2, 1] {
            let got = check(dir.path(), &sealed, "0000", || wiped.set(wiped.get() + 1));
            assert!(
                matches!(got, Check::Wrong { left } if left == expect_left),
                "{got:?}"
            );
            assert_eq!(wiped.get(), 0, "not wiped yet");
        }
        let fifth = check(dir.path(), &sealed, "0000", || wiped.set(wiped.get() + 1));
        assert!(matches!(fifth, Check::Wiped), "{fifth:?}");
        assert_eq!(wiped.get(), 1, "wiped exactly once");
        assert_eq!(count(dir.path()), 0, "a wiped PIN starts over");
    }

    #[test]
    fn a_right_pin_resets_the_count() {
        let dir = tempfile::tempdir().unwrap();
        let sealed = blob("vault pass", "4821");
        for _ in 0..4 {
            check(dir.path(), &sealed, "1111", || panic!("no wipe yet"));
        }
        assert_eq!(count(dir.path()), 4);
        let ok = check(dir.path(), &sealed, "4821", || {
            panic!("a right PIN wipes nothing")
        });
        assert!(
            matches!(&ok, Check::Passphrase(p) if p.expose_secret() == "vault pass"),
            "{ok:?}"
        );
        assert_eq!(count(dir.path()), 0);
        // Four more wrong ones are four, not eight: still no wipe.
        for _ in 0..4 {
            check(dir.path(), &sealed, "1111", || {
                panic!("count did not reset")
            });
        }
    }

    #[test]
    fn the_count_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let sealed = blob("p", "4821");
        check(dir.path(), &sealed, "1", || {});
        check(dir.path(), &sealed, "2", || {});
        // A new process sees what the file says, not zero.
        assert_eq!(count(dir.path()), 2);
        let got = check(dir.path(), &sealed, "3", || {});
        assert!(matches!(got, Check::Wrong { left: 2 }), "{got:?}");
    }

    #[test]
    fn the_count_is_written_before_the_pin_is_checked() {
        // A killed check must not be a free guess: after a try that never finished, the file
        // already says one.
        let dir = tempfile::tempdir().unwrap();
        store(dir.path(), 4);
        let sealed = blob("p", "4821");
        let got = check(dir.path(), &sealed, "9999", || {});
        assert!(
            matches!(got, Check::Wiped),
            "4 recorded + this one is 5: {got:?}"
        );
    }

    #[test]
    fn verdicts_follow_the_count() {
        assert_eq!(verdict(1, false), Attempt::Wrong { left: 4 });
        assert_eq!(verdict(4, false), Attempt::Wrong { left: 1 });
        assert_eq!(verdict(5, false), Attempt::Wipe);
        assert_eq!(verdict(9, false), Attempt::Wipe);
        assert_eq!(verdict(5, true), Attempt::Right);
    }

    #[derive(Default)]
    struct Mem(RefCell<Option<Vec<u8>>>);

    impl Store for Mem {
        fn load(&self) -> Option<Vec<u8>> {
            self.0.borrow().clone()
        }
        fn save(&self, blob: &[u8]) -> Result<(), String> {
            *self.0.borrow_mut() = Some(blob.to_vec());
            Ok(())
        }
        fn delete(&self) -> Result<(), String> {
            *self.0.borrow_mut() = None;
            Ok(())
        }
    }

    /// A saved vault under `pass` holding one password, and a PIN sealed to it.
    fn vault_with_pin(dir: &Path, pass: &str, pin_text: &str, store: &Mem) -> std::path::PathBuf {
        let path = dir.join("vault.age");
        let mut vault = Vault::new(SecretString::from(pass.to_owned()));
        vault.set(
            tern_vault::Key::Password {
                user: "test".into(),
                host: "h".into(),
                port: 22,
            },
            SecretString::from("hunter2".to_owned()),
        );
        vault.save(&path).unwrap();
        set(dir, vault.passphrase(), pin_text, store).unwrap();
        path
    }

    #[test]
    fn the_pin_opens_the_vault_and_a_wrong_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let store = Mem::default();
        let path = vault_with_pin(dir.path(), "long passphrase", "4821", &store);
        let Outcome::Opened(v) = unlock(dir.path(), &path, "4821", &store) else {
            panic!("the right PIN must open it");
        };
        assert_eq!(v.len(), 1);
        assert!(matches!(
            unlock(dir.path(), &path, "4822", &store),
            Outcome::Wrong { left: 4 }
        ));
        // Nothing sealed, nothing opens: the PIN alone never reaches the vault file.
        store.delete().unwrap();
        assert!(matches!(
            unlock(dir.path(), &path, "4821", &store),
            Outcome::Gone
        ));
    }

    #[test]
    fn five_wrong_pins_delete_the_item_and_even_the_right_pin_then_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = Mem::default();
        let path = vault_with_pin(dir.path(), "long passphrase", "4821", &store);
        for _ in 0..4 {
            assert!(matches!(
                unlock(dir.path(), &path, "0000", &store),
                Outcome::Wrong { .. }
            ));
        }
        assert!(store.load().is_some());
        assert!(matches!(
            unlock(dir.path(), &path, "0000", &store),
            Outcome::Wiped
        ));
        assert!(store.load().is_none(), "the sealed passphrase is deleted");
        assert!(matches!(
            unlock(dir.path(), &path, "4821", &store),
            Outcome::Gone
        ));
    }

    #[test]
    fn changing_the_vault_passphrase_clears_the_pin() {
        let dir = tempfile::tempdir().unwrap();
        let store = Mem::default();
        let path = vault_with_pin(dir.path(), "old passphrase", "4821", &store);
        super::store(dir.path(), 3);
        Vault::change_passphrase(
            &path,
            SecretString::from("old passphrase".to_owned()),
            SecretString::from("new passphrase".to_owned()),
        )
        .unwrap();
        clear(dir.path(), &store).unwrap();
        assert!(store.load().is_none());
        assert_eq!(count(dir.path()), 0);
        assert!(matches!(
            unlock(dir.path(), &path, "4821", &store),
            Outcome::Gone
        ));
    }

    #[test]
    fn a_sealed_passphrase_the_vault_no_longer_takes_is_dropped_not_retried() {
        // The vault was re-sealed elsewhere (sync) without going through the app's change.
        let dir = tempfile::tempdir().unwrap();
        let store = Mem::default();
        let path = vault_with_pin(dir.path(), "old passphrase", "4821", &store);
        Vault::change_passphrase(
            &path,
            SecretString::from("old passphrase".to_owned()),
            SecretString::from("new passphrase".to_owned()),
        )
        .unwrap();
        assert!(matches!(
            unlock(dir.path(), &path, "4821", &store),
            Outcome::Gone
        ));
        assert!(store.load().is_none());
    }

    #[test]
    fn a_pin_that_is_not_digits_is_refused_before_anything_is_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = Mem::default();
        let pass = SecretString::from("p".to_owned());
        for bad in ["12", "abcd", "123456789", ""] {
            assert!(set(dir.path(), &pass, bad, &store).is_err(), "{bad:?}");
            assert!(store.load().is_none());
        }
    }

    #[test]
    fn messages_say_how_many_tries_are_left() {
        assert_eq!(
            message(&Outcome::Wrong { left: 4 }).unwrap(),
            "Wrong PIN, 4 tries left."
        );
        assert_eq!(
            message(&Outcome::Wrong { left: 1 }).unwrap(),
            "Wrong PIN, 1 try left."
        );
        assert_eq!(
            message(&Outcome::Wiped).unwrap(),
            "Too many wrong PINs — PIN unlock is off; use your passphrase"
        );
    }
}
