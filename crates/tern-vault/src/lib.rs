//! One encrypted file holding SSH secrets, unlocked with a passphrase.
//!
//! The feature is Tabby's vault (`tabby-core/src/services/vault.service.ts`): passwords per
//! `user@host:port`, passphrases per key file. The crypto is not Tabby's (PBKDF2 + AES-CBC with
//! no MAC, so tampering goes unnoticed): the file is an [age] file with an scrypt passphrase
//! recipient, which is authenticated, so a wrong passphrase and a tampered file are both refused.
//!
//! Plaintext exists only inside this crate, in buffers zeroed on drop; callers get
//! [`SecretString`]s.

use std::io;
use std::path::Path;

pub use age::secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// What a secret unlocks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Key {
    /// A login password for `user@host:port`.
    Password {
        user: String,
        host: String,
        port: u16,
    },
    /// The passphrase of a private key file, by its path.
    KeyPassphrase { path: String },
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("wrong passphrase, or the vault file was modified")]
    WrongPassphrase,
    #[error("vault file is not a tern vault: {0}")]
    Corrupt(String),
    #[error("vault file: {0}")]
    Io(#[from] io::Error),
}

/// On-disk entry. `value` is plaintext only between decryption and conversion, and is zeroed
/// on drop.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct Entry {
    #[zeroize(skip)]
    key: Key,
    value: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Contents {
    version: u32,
    entries: Vec<Entry>,
}

const VERSION: u32 = 1;
/// scrypt cost, log2(N). age's own default targets about a second; 18 is that on current Macs.
const WORK_FACTOR: u8 = 18;

/// An unlocked vault: secrets in memory, the passphrase kept to re-encrypt on save.
pub struct Vault {
    passphrase: SecretString,
    entries: Vec<(Key, SecretString)>,
    work_factor: u8,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// An empty vault that will be encrypted with `passphrase`.
    pub fn new(passphrase: SecretString) -> Self {
        Self {
            passphrase,
            entries: Vec::new(),
            work_factor: WORK_FACTOR,
        }
    }

    /// Decrypts the vault at `path`.
    ///
    /// # Errors
    /// [`VaultError::WrongPassphrase`] when the passphrase does not open it or the file was
    /// altered; [`VaultError::Corrupt`] when it is not an age file or holds unexpected data;
    /// [`VaultError::Io`] when it cannot be read.
    pub fn unlock(path: &Path, passphrase: SecretString) -> Result<Self, VaultError> {
        let ciphertext = std::fs::read(path)?;
        let identity = age::scrypt::Identity::new(passphrase.clone());
        let plaintext =
            Zeroizing::new(age::decrypt(&identity, &ciphertext).map_err(|e| match e {
                age::DecryptError::DecryptionFailed
                | age::DecryptError::KeyDecryptionFailed
                | age::DecryptError::NoMatchingKeys
                | age::DecryptError::InvalidMac => VaultError::WrongPassphrase,
                other => VaultError::Corrupt(other.to_string()),
            })?);
        let mut contents: Contents =
            serde_json::from_slice(&plaintext).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        if contents.version != VERSION {
            return Err(VaultError::Corrupt(format!(
                "version {} is not {VERSION}",
                contents.version
            )));
        }
        let entries = contents
            .entries
            .iter_mut()
            .map(|entry| {
                let value = SecretString::from(std::mem::take(&mut entry.value));
                (entry.key.clone(), value)
            })
            .collect();
        Ok(Self {
            passphrase,
            entries,
            work_factor: WORK_FACTOR,
        })
    }

    /// Encrypts and writes the vault to `path` via a temp file and rename, so an interrupted
    /// save leaves the previous vault intact.
    ///
    /// # Errors
    /// [`VaultError::Io`] when the file cannot be written; [`VaultError::Corrupt`] if encoding
    /// fails, which would be a bug.
    pub fn save(&self, path: &Path) -> Result<(), VaultError> {
        let contents = Contents {
            version: VERSION,
            entries: self
                .entries
                .iter()
                .map(|(key, value)| Entry {
                    key: key.clone(),
                    value: value.expose_secret().to_owned(),
                })
                .collect(),
        };
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&contents).map_err(|e| VaultError::Corrupt(e.to_string()))?,
        );
        let mut recipient = age::scrypt::Recipient::new(self.passphrase.clone());
        recipient.set_work_factor(self.work_factor);
        let ciphertext =
            age::encrypt(&recipient, &plaintext).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("age.tmp");
        std::fs::write(&tmp, ciphertext)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn get(&self, key: &Key) -> Option<&SecretString> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Stores `value` under `key`, replacing an earlier one.
    pub fn set(&mut self, key: Key, value: SecretString) {
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some((_, slot)) => *slot = value,
            None => self.entries.push((key, value)),
        }
    }

    pub fn remove(&mut self, key: &Key) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(k, _)| k != key);
        self.entries.len() != before
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Cheaper scrypt for tests; a real vault keeps [`WORK_FACTOR`].
    #[cfg(test)]
    fn with_work_factor(mut self, log_n: u8) -> Self {
        self.work_factor = log_n;
        self
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
