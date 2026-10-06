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
