//! The vault passphrase in the macOS login Keychain, for the opt-in "unlock at launch" setting.
//!
//! One generic-password item, service `tern vault` (`tern vault (debug)` in debug builds, so a
//! development run never touches the real item). The passphrase goes nowhere else and is never
//! logged: errors carry the Keychain status text only.

use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};
use tern_vault::{ExposeSecret, SecretString};

/// `errSecItemNotFound`.
const NOT_FOUND: i32 = -25300;
const ACCOUNT: &str = "passphrase";

pub fn service() -> &'static str {
    if cfg!(debug_assertions) {
        "tern vault (debug)"
    } else {
        "tern vault"
    }
}

/// Stores or replaces the passphrase.
pub fn store(passphrase: &SecretString) -> Result<(), String> {
    set_generic_password(service(), ACCOUNT, passphrase.expose_secret().as_bytes())
        .map_err(|e| format!("Keychain: {e}"))
}

/// The stored passphrase; `None` when there is none or the Keychain refuses.
pub fn load() -> Option<SecretString> {
    match get_generic_password(service(), ACCOUNT) {
        Ok(bytes) => String::from_utf8(bytes).ok().map(SecretString::from),
        Err(e) => {
            if e.code() != NOT_FOUND {
                tracing::warn!(status = e.code(), "vault_keychain_read_failed");
            }
            None
        }
    }
}

/// Removes the item; an item that is already gone counts as removed.
pub fn delete() -> Result<(), String> {
    match delete_generic_password(service(), ACCOUNT) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == NOT_FOUND => Ok(()),
        Err(e) => Err(format!("Keychain: {e}")),
    }
}
