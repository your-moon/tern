//! One encrypted file holding SSH secrets, unlocked with a passphrase.
//!
//! The feature is Tabby's vault (`tabby-core/src/services/vault.service.ts`): passwords per
//! `user@host:port`, passphrases per key file. The crypto is not Tabby's (PBKDF2 + AES-CBC with
//! no MAC, so tampering goes unnoticed): the file is an [age] file with an scrypt passphrase
//! recipient, which is authenticated, so a wrong passphrase and a tampered file are both refused.
//!
//! Plaintext exists only inside this crate, in buffers zeroed on drop; callers get
//! [`SecretString`]s.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

pub mod keys;
pub mod pin;

pub use age::secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
pub use zeroize::Zeroizing;
use zeroize::{Zeroize, ZeroizeOnDrop};

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
    /// A private key kept in the vault, by the name the person gave it. The value is the key
    /// in OpenSSH text form.
    SshKey { name: String },
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

/// A new file only the owner can read.
fn private_file(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

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
        Self::unlock_bytes(&std::fs::read(path)?, passphrase)
    }

    /// [`Vault::unlock`] for bytes already in memory, such as a vault downloaded by sync.
    ///
    /// # Errors
    /// As [`Vault::unlock`], without the I/O case.
    pub fn unlock_bytes(ciphertext: &[u8], passphrase: SecretString) -> Result<Self, VaultError> {
        let identity = age::scrypt::Identity::new(passphrase.clone());
        let plaintext =
            Zeroizing::new(age::decrypt(&identity, ciphertext).map_err(|e| match e {
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
        let tmp = self.stage(path)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// First half of [`Vault::save`]: the new vault, complete and flushed, in a temp file next
    /// to `path`. Until the rename, `path` still holds the old vault.
    fn stage(&self, path: &Path) -> Result<PathBuf, VaultError> {
        let ciphertext = self.to_bytes()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("age.tmp");
        let mut file = private_file(&tmp)?;
        file.write_all(&ciphertext)?;
        // On disk before the rename, or a crash could leave the new name on empty data.
        file.sync_all()?;
        Ok(tmp)
    }

    /// Opens the vault at `path` with `current`, re-seals it under `new` and replaces the file
    /// (temp file and rename). Returns the open vault, now under `new`.
    ///
    /// # Errors
    /// [`VaultError::WrongPassphrase`] when `current` does not open the file, in which case
    /// nothing is written; otherwise as [`Vault::unlock`] and [`Vault::save`].
    pub fn change_passphrase(
        path: &Path,
        current: SecretString,
        new: SecretString,
    ) -> Result<Vault, VaultError> {
        Self::change_passphrase_with(path, current, new, WORK_FACTOR)
    }

    fn change_passphrase_with(
        path: &Path,
        current: SecretString,
        new: SecretString,
        work_factor: u8,
    ) -> Result<Vault, VaultError> {
        let mut vault = Self::unlock(path, current)?;
        vault.passphrase = new;
        vault.work_factor = work_factor;
        vault.save(path)?;
        Ok(vault)
    }

    /// The encrypted vault file, as [`Vault::save`] writes it.
    ///
    /// # Errors
    /// [`VaultError::Corrupt`] if encoding fails, which would be a bug.
    pub fn to_bytes(&self) -> Result<Vec<u8>, VaultError> {
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
        self.seal(&plaintext)
    }

    /// Opens another vault file sealed with the same passphrase (a synced copy).
    ///
    /// # Errors
    /// As [`Vault::unlock_bytes`].
    pub fn reopen(&self, ciphertext: &[u8]) -> Result<Vault, VaultError> {
        Self::unlock_bytes(ciphertext, self.passphrase.clone())
    }

    /// Encrypts any bytes with this vault's passphrase, for data that travels with the vault
    /// (sync uploads hosts and settings this way, so nothing readable leaves the machine).
    ///
    /// # Errors
    /// [`VaultError::Corrupt`] if encryption fails, which would be a bug.
    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
        let mut recipient = age::scrypt::Recipient::new(self.passphrase.clone());
        recipient.set_work_factor(self.work_factor);
        age::encrypt(&recipient, plaintext).map_err(|e| VaultError::Corrupt(e.to_string()))
    }

    /// Reverses [`Vault::seal`].
    ///
    /// # Errors
    /// [`VaultError::WrongPassphrase`] when sealed under another passphrase or altered.
    pub fn open(&self, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        let identity = age::scrypt::Identity::new(self.passphrase.clone());
        age::decrypt(&identity, ciphertext)
            .map(Zeroizing::new)
            .map_err(|_| VaultError::WrongPassphrase)
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

    /// Every key with a secret, in the order they were added. Values stay inside.
    pub fn keys(&self) -> impl Iterator<Item = &Key> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// The passphrase the vault is sealed with, for the Keychain opt-in. Callers must not log
    /// or store it anywhere else.
    pub fn passphrase(&self) -> &SecretString {
        &self.passphrase
    }

    /// An independent copy, so a save can run off the UI thread while the original stays
    /// editable.
    pub fn snapshot(&self) -> Vault {
        Vault {
            passphrase: self.passphrase.clone(),
            entries: self.entries.clone(),
            work_factor: self.work_factor,
        }
    }

    /// Drops every secret and the passphrase now, rather than whenever the value goes away.
    /// They are [`SecretString`]s, zeroed as they are dropped.
    pub fn wipe(&mut self) {
        self.entries.clear();
        self.passphrase = SecretString::from(String::new());
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
