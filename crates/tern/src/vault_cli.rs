//! `tern vault init`: creates the vault without the window, reading the passphrase and a PIN
//! from standard input (two lines), so a password manager can pipe them in and nothing lands
//! in shell history or a file. It also turns on Keychain unlock, as Settings → Vault does.

use std::io::BufRead;
use std::path::Path;

use tern_vault::{SecretString, Vault};

use crate::{keychain, settings, vault_pin};

/// Runs `tern vault init` against tern's settings directory.
///
/// # Errors
/// A message for the person: a vault already exists, input is missing, the PIN is not 4–8
/// digits, or the Keychain refuses.
pub fn init(input: impl BufRead) -> Result<String, String> {
    let dir = settings::dir().ok_or("no settings directory")?;
    init_in(&dir, input, &vault_pin::KeychainStore, &|p| {
        keychain::store(p)
    })
}

fn init_in(
    dir: &Path,
    input: impl BufRead,
    pins: &impl vault_pin::Store,
    unlock: &dyn Fn(&SecretString) -> Result<(), String>,
) -> Result<String, String> {
    let path = dir.join("vault.age");
    if path.exists() {
        return Err(format!(
            "a vault already exists at {}; change it from Settings → Vault",
            path.display()
        ));
    }
    let mut lines = input.lines();
    let mut next = |what: &str| {
        lines
            .next()
            .and_then(Result::ok)
            .map(|l| l.trim_end_matches('\r').to_owned())
            .filter(|l| !l.is_empty())
            .ok_or(format!("expected the {what} on standard input"))
    };
    let passphrase = SecretString::from(next("passphrase")?);
    let pin = next("PIN")?;
    if let Some(problem) = tern_vault::pin::problem(&pin) {
        return Err(problem.into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    Vault::new(passphrase.clone())
        .save(&path)
        .map_err(|e| e.to_string())?;
    vault_pin::set(dir, &passphrase, &pin, pins)?;
    unlock(&passphrase)?;
    let mut s = settings::Settings::load(dir);
    s.vault_keychain = true;
    s.save(dir).map_err(|e| e.to_string())?;
    Ok(format!(
        "vault created at {}; PIN unlock and Keychain unlock are on",
        path.display()
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::cell::RefCell;

    use tern_vault::ExposeSecret;
    use vault_pin::Store;

    use super::*;

    #[derive(Default)]
    struct Mem(RefCell<Option<Vec<u8>>>);
    impl vault_pin::Store for Mem {
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

    #[test]
    fn creates_a_vault_the_passphrase_opens_and_seals_it_to_the_pin() {
        let dir = tempfile::tempdir().unwrap();
        let pins = Mem::default();
        let unlocked = RefCell::new(None);
        let out = init_in(
            dir.path(),
            "correct horse battery\n1112\n".as_bytes(),
            &pins,
            &|p| {
                *unlocked.borrow_mut() = Some(p.expose_secret().to_owned());
                Ok(())
            },
        );
        assert!(out.is_ok(), "{out:?}");
        let path = dir.path().join("vault.age");
        assert!(
            Vault::unlock(
                &path,
                SecretString::from("correct horse battery".to_owned())
            )
            .is_ok()
        );
        assert!(Vault::unlock(&path, SecretString::from("1112".to_owned())).is_err());
        let blob = pins.load().unwrap();
        let back = tern_vault::pin::unwrap(&blob, "1112").unwrap();
        assert_eq!(back.expose_secret(), "correct horse battery");
        assert_eq!(unlocked.borrow().as_deref(), Some("correct horse battery"));
        assert!(settings::Settings::load(dir.path()).vault_keychain);
    }

    #[test]
    fn refuses_an_existing_vault_a_bad_pin_and_missing_input() {
        let dir = tempfile::tempdir().unwrap();
        let ok = |_: &SecretString| Ok(());
        std::fs::write(dir.path().join("vault.age"), b"x").unwrap();
        assert!(init_in(dir.path(), "p\n1112\n".as_bytes(), &Mem::default(), &ok).is_err());
        let fresh = tempfile::tempdir().unwrap();
        assert!(init_in(fresh.path(), "p\n12\n".as_bytes(), &Mem::default(), &ok).is_err());
        assert!(init_in(fresh.path(), "p\n".as_bytes(), &Mem::default(), &ok).is_err());
        assert!(
            !fresh.path().join("vault.age").exists(),
            "nothing written on bad input"
        );
    }
}
