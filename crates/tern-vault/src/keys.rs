//! SSH private keys kept in the vault: generating ed25519 keys, and reading a key's public half.
//!
//! Keys are stored as OpenSSH-format text (`-----BEGIN OPENSSH PRIVATE KEY-----`), the same
//! text `ssh-keygen` writes, so a key can be exported and used by any other tool. The `ssh-key`
//! crate is the one russh already uses, so tern adds no second implementation.

use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey};

use crate::SecretString;

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("this is not an OpenSSH private key")]
    NotAKey,
    #[error("this key has its own passphrase; remove it first (ssh-keygen -p -N \"\" -f <key>)")]
    Encrypted,
    #[error("could not make a key: {0}")]
    Generate(String),
}

/// What a person needs to see about a key: never the private half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// `ssh-ed25519`, `ssh-rsa`, …
    pub algorithm: String,
    /// The one-line public key, ready for `authorized_keys`.
    pub public: String,
    /// `SHA256:…`, as `ssh-keygen -l` prints.
    pub fingerprint: String,
}

/// A fresh ed25519 key: the private text for the vault and what to show.
#[derive(Debug)]
pub struct Generated {
    pub private: SecretString,
    pub info: KeyInfo,
}

/// Makes an ed25519 key; `comment` ends up on the public line (`tern@host`, a label).
///
/// # Errors
/// [`KeyError::Generate`] if the key cannot be made or encoded, which would be a bug.
pub fn generate_ed25519(comment: &str) -> Result<Generated, KeyError> {
    let mut key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .map_err(|e| KeyError::Generate(e.to_string()))?;
    key.set_comment(comment);
    let text = key
        .to_openssh(LineEnding::LF)
        .map_err(|e| KeyError::Generate(e.to_string()))?;
    let info = describe(&key)?;
    // `text` is zeroed on drop; the vault takes its own copy.
    Ok(Generated {
        private: SecretString::from(text.as_str().to_owned()),
        info,
    })
}

/// Checks that `text` is a usable OpenSSH private key and describes it.
///
/// # Errors
/// [`KeyError::NotAKey`] for anything that is not OpenSSH private-key text;
/// [`KeyError::Encrypted`] for a key protected by its own passphrase, which the vault would
/// have to ask for at every login.
pub fn inspect(text: &str) -> Result<KeyInfo, KeyError> {
    let key = PrivateKey::from_openssh(text).map_err(|_| KeyError::NotAKey)?;
    if key.is_encrypted() {
        return Err(KeyError::Encrypted);
    }
    describe(&key)
}

fn describe(key: &PrivateKey) -> Result<KeyInfo, KeyError> {
    let public = key
        .public_key()
        .to_openssh()
        .map_err(|e| KeyError::Generate(e.to_string()))?;
    Ok(KeyInfo {
        algorithm: key.algorithm().to_string(),
        public,
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
    })
}
