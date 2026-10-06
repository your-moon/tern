//! The vault passphrase in the platform credential store, for the opt-in "unlock at launch"
//! setting: the login Keychain on macOS, Credential Manager on Windows, the Secret Service
//! (GNOME Keyring, KWallet) on Linux.
//!
//! One generic-password item, service `tern vault` (`tern vault (debug)` in debug builds, so a
//! development run never touches the real item). The passphrase goes nowhere else and is never
//! logged: errors carry the store's status text only.

use tern_vault::{ExposeSecret, SecretString};

const ACCOUNT: &str = "passphrase";

/// The native store, behind three calls. `None` from `get` means "no such item".
#[cfg(target_os = "macos")]
mod backend {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    pub fn set(service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
        set_generic_password(service, account, secret).map_err(|e| e.to_string())
    }

    pub fn get(service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
        match get_generic_password(service, account) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete(service: &str, account: &str) -> Result<(), String> {
        match delete_generic_password(service, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod backend {
    fn entry(service: &str, account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(service, account).map_err(|e| e.to_string())
    }

    pub fn set(service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
        entry(service, account)?
            .set_secret(secret)
            .map_err(|e| e.to_string())
    }

    pub fn get(service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
        match entry(service, account)?.get_secret() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete(service: &str, account: &str) -> Result<(), String> {
        match entry(service, account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// What the store is called on this OS, for messages.
pub const STORE_NAME: &str = if cfg!(target_os = "macos") {
    "Keychain"
} else if cfg!(target_os = "windows") {
    "Credential Manager"
} else {
    "Secret Service"
};

fn set(service: &str, secret: &[u8]) -> Result<(), String> {
    backend::set(service, ACCOUNT, secret).map_err(|e| format!("{STORE_NAME}: {e}"))
}

/// The item's bytes; `None` when there is none or the store refuses (logged under `event`).
fn get(service: &str, event: &'static str) -> Option<Vec<u8>> {
    match backend::get(service, ACCOUNT) {
        Ok(found) => found,
        Err(e) => {
            tracing::warn!(error = %e, event);
            None
        }
    }
}

/// Removes an item; one that is already gone counts as removed.
fn remove(service: &str) -> Result<(), String> {
    backend::delete(service, ACCOUNT).map_err(|e| format!("{STORE_NAME}: {e}"))
}

pub fn service() -> &'static str {
    if cfg!(debug_assertions) {
        "tern vault (debug)"
    } else {
        "tern vault"
    }
}

/// Stores or replaces the passphrase.
pub fn store(passphrase: &SecretString) -> Result<(), String> {
    set(service(), passphrase.expose_secret().as_bytes())
}

/// The stored passphrase; `None` when there is none or the store refuses.
pub fn load() -> Option<SecretString> {
    get(service(), "vault_keychain_read_failed")
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(SecretString::from)
}

/// Removes the item; an item that is already gone counts as removed.
pub fn delete() -> Result<(), String> {
    remove(service())
}

// ---- the vault PIN ------------------------------------------------------------------------

/// The vault passphrase sealed to the PIN (see `vault_pin`), in its own item: service
/// `tern vault pin` (`tern vault pin (debug)` in debug builds). The PIN opens nothing without
/// this item, and the item is useless without the PIN.
pub fn pin_service() -> &'static str {
    if cfg!(debug_assertions) {
        "tern vault pin (debug)"
    } else {
        "tern vault pin"
    }
}

pub fn store_pin_blob(blob: &[u8]) -> Result<(), String> {
    set(pin_service(), blob)
}

pub fn load_pin_blob() -> Option<Vec<u8>> {
    get(pin_service(), "vault_pin_keychain_read_failed")
}

/// Removes the PIN item; one that is already gone counts as removed.
pub fn delete_pin_blob() -> Result<(), String> {
    remove(pin_service())
}
