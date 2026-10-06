use super::*;

fn pass(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn login(host: &str) -> Key {
    Key::Password {
        user: "deploy".into(),
        host: host.into(),
        port: 22,
    }
}

fn saved(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("vault.age");
    let mut v = Vault::new(pass("correct horse")).with_work_factor(4);
    v.set(login("grape"), pass("hunter2"));
    v.set(
        Key::KeyPassphrase {
            path: "~/.ssh/id_ed25519".into(),
        },
        pass("key-pass"),
    );
    v.save(&path).unwrap();
    path
}

#[test]
fn round_trip_keeps_every_secret_under_its_own_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved(&dir);
    let v = Vault::unlock(&path, pass("correct horse")).unwrap();
    assert_eq!(v.len(), 2);
    assert_eq!(v.get(&login("grape")).unwrap().expose_secret(), "hunter2");
    let key = Key::KeyPassphrase {
        path: "~/.ssh/id_ed25519".into(),
    };
    assert_eq!(v.get(&key).unwrap().expose_secret(), "key-pass");
    assert!(v.get(&login("strong-b")).is_none());
}

#[test]
fn file_does_not_contain_the_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = std::fs::read(saved(&dir)).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("hunter2") && !text.contains("grape"));
    assert!(text.starts_with("age-encryption.org/v1"));
}

#[test]
fn wrong_passphrase_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved(&dir);
    let err = Vault::unlock(&path, pass("correct horse!")).unwrap_err();
    assert!(matches!(err, VaultError::WrongPassphrase), "{err:?}");
}

#[test]
fn a_flipped_byte_in_the_body_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved(&dir);
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(&path, bytes).unwrap();
    assert!(Vault::unlock(&path, pass("correct horse")).is_err());
}

#[test]
fn set_replaces_and_remove_deletes() {
    let mut v = Vault::new(pass("p"));
    v.set(login("grape"), pass("one"));
    v.set(login("grape"), pass("two"));
    assert_eq!(v.len(), 1);
    assert_eq!(v.get(&login("grape")).unwrap().expose_secret(), "two");
    assert!(v.remove(&login("grape")));
    assert!(!v.remove(&login("grape")));
    assert!(v.is_empty());
}

#[test]
fn not_an_age_file_is_corrupt_not_wrong_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.age");
    std::fs::write(&path, b"{\"version\":1}").unwrap();
    let err = Vault::unlock(&path, pass("x")).unwrap_err();
    assert!(matches!(err, VaultError::Corrupt(_)), "{err:?}");
}

#[test]
fn debug_never_shows_secrets() {
    let mut v = Vault::new(pass("master"));
    v.set(login("grape"), pass("hunter2"));
    let shown = format!("{v:?}");
    assert!(!shown.contains("hunter2") && !shown.contains("master"));
}

#[test]
fn sealed_data_opens_only_with_the_same_passphrase() {
    let v = Vault::new(pass("correct horse")).with_work_factor(4);
    let sealed = v.seal(b"hosts.json").unwrap();
    assert!(!String::from_utf8_lossy(&sealed).contains("hosts.json"));
    assert_eq!(&*v.open(&sealed).unwrap(), b"hosts.json");
    let other = Vault::new(pass("battery staple")).with_work_factor(4);
    assert!(matches!(
        other.open(&sealed),
        Err(VaultError::WrongPassphrase)
    ));
}

fn ssh_key_entry(name: &str) -> Key {
    Key::SshKey { name: name.into() }
}

#[test]
fn changed_passphrase_opens_the_vault_and_the_old_one_no_longer_does() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved(&dir);
    let rekeyed =
        Vault::change_passphrase_with(&path, pass("correct horse"), pass("battery staple"), 4)
            .unwrap();
    assert_eq!(rekeyed.len(), 2);
    let again = Vault::unlock(&path, pass("battery staple")).unwrap();
    assert_eq!(
        again.get(&login("grape")).unwrap().expose_secret(),
        "hunter2"
    );
    let err = Vault::unlock(&path, pass("correct horse")).unwrap_err();
    assert!(matches!(err, VaultError::WrongPassphrase), "{err:?}");
}

#[test]
fn wrong_current_passphrase_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved(&dir);
    let before = std::fs::read(&path).unwrap();
    let err = Vault::change_passphrase_with(&path, pass("not it"), pass("new one"), 4).unwrap_err();
    assert!(matches!(err, VaultError::WrongPassphrase), "{err:?}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(Vault::unlock(&path, pass("correct horse")).is_ok());
}

#[test]
fn a_crash_between_write_and_rename_leaves_the_old_vault_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved(&dir);
    let mut next = Vault::unlock(&path, pass("correct horse"))
        .unwrap()
        .with_work_factor(4);
    next.passphrase = pass("battery staple");
    next.set(login("strong-b"), pass("added after"));
    // The process dies here: the new file is fully written but never renamed.
    let tmp = next.stage(&path).unwrap();
    assert!(tmp.exists());
    let old = Vault::unlock(&path, pass("correct horse")).unwrap();
    assert_eq!(old.len(), 2, "old vault must be untouched");
    assert!(Vault::unlock(&path, pass("battery staple")).is_err());
    // Finishing the rename is what makes the new vault current.
    std::fs::rename(&tmp, &path).unwrap();
    let new = Vault::unlock(&path, pass("battery staple")).unwrap();
    assert_eq!(new.len(), 3);
}

#[cfg(unix)]
#[test]
fn saved_file_is_private_to_the_owner() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let path = saved(&dir);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "mode {mode:o}");
}

#[test]
fn wipe_forgets_every_secret_and_the_passphrase() {
    let mut v = Vault::new(pass("master")).with_work_factor(4);
    v.set(login("grape"), pass("hunter2"));
    v.set(ssh_key_entry("laptop"), pass("pem"));
    v.wipe();
    assert!(v.is_empty());
    assert!(v.get(&login("grape")).is_none());
    assert_eq!(v.passphrase().expose_secret(), "");
}

#[test]
fn snapshot_is_independent_of_the_original() {
    let mut v = Vault::new(pass("master"));
    v.set(login("grape"), pass("hunter2"));
    let copy = v.snapshot();
    v.remove(&login("grape"));
    assert_eq!(
        copy.get(&login("grape")).unwrap().expose_secret(),
        "hunter2"
    );
}

#[test]
fn keys_lists_every_entry_kind_in_insertion_order() {
    let mut v = Vault::new(pass("p"));
    v.set(login("grape"), pass("hunter2"));
    v.set(ssh_key_entry("laptop"), pass("pem"));
    let keys: Vec<_> = v.keys().cloned().collect();
    assert_eq!(keys, vec![login("grape"), ssh_key_entry("laptop")]);
}

#[test]
fn generated_key_round_trips_through_the_vault_and_reports_its_public_half() {
    let g = keys::generate_ed25519("tern@test").unwrap();
    assert!(
        g.private
            .expose_secret()
            .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
    );
    assert!(g.info.public.starts_with("ssh-ed25519 "));
    assert!(g.info.public.ends_with(" tern@test"));
    assert!(g.info.fingerprint.starts_with("SHA256:"));
    // Two keys are never the same key.
    let other = keys::generate_ed25519("tern@test").unwrap();
    assert_ne!(g.info.public, other.info.public);

    let mut v = Vault::new(pass("p")).with_work_factor(4);
    let key = ssh_key_entry("laptop");
    v.set(key.clone(), g.private);
    let bytes = v.to_bytes().unwrap();
    let back = v.reopen(&bytes).unwrap();
    let info = keys::inspect(back.get(&key).unwrap().expose_secret()).unwrap();
    assert_eq!(info, g.info);
}

#[test]
fn inspect_refuses_text_that_is_not_a_usable_key() {
    assert!(matches!(
        keys::inspect("hello"),
        Err(keys::KeyError::NotAKey)
    ));
    let g = keys::generate_ed25519("x").unwrap();
    // The public line is not a private key.
    assert!(matches!(
        keys::inspect(&g.info.public),
        Err(keys::KeyError::NotAKey)
    ));
}
