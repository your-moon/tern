//! The app's one vault: where it lives and, once unlocked, the open [`Vault`] every tab reads.
//! Unlocking and saving run scrypt (about a second), so both happen off the UI thread.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use gpui::{App, AppContext, Global, Task};
use tern_ssh::{ExposeSecret, Prompt, SecretString};
use tern_vault::{Key, Vault, VaultError};

pub struct Keeper {
    path: Option<PathBuf>,
    open: Option<Vault>,
    /// The Keychain opt-in (Settings -> Vault) is on.
    keychain: bool,
    /// The Keychain item already holds the current passphrase, so `put` need not write it.
    keychain_current: bool,
    /// Numbers each write in the order it was asked for.
    ticket: u64,
}

impl Global for Keeper {}

/// The newest write that reached the disk. Writes run on background threads and could finish
/// out of order; an older one must never overwrite a newer one.
static WRITTEN: Mutex<u64> = Mutex::new(0);

/// Runs `write` only if no newer write (higher `ticket`) has already run; `None` when skipped.
fn in_order<T>(ticket: u64, write: impl FnOnce() -> T) -> Option<T> {
    let mut last = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    if ticket < *last {
        return None;
    }
    *last = ticket;
    Some(write())
}

impl Keeper {
    pub fn install(keychain: bool, cx: &mut App) {
        let path = crate::settings::dir().map(|d| d.join("vault.age"));
        cx.set_global(Self {
            path,
            open: None,
            keychain,
            keychain_current: false,
            ticket: 0,
        });
    }

    /// Opens the vault at launch with the passphrase from the Keychain, when that is on. A
    /// missing item, a refusal or a passphrase changed elsewhere leaves it locked and says
    /// nothing: the first lookup then asks as usual.
    pub fn unlock_from_keychain(cx: &mut App) {
        let k = cx.global::<Self>();
        let (true, Some(path)) = (k.keychain, k.path.clone()) else {
            return;
        };
        if !path.exists() {
            return;
        }
        let work = cx.background_spawn(async move {
            let passphrase = crate::keychain::load()?;
            Vault::unlock(&path, passphrase).ok()
        });
        cx.spawn(async move |cx| {
            if let Some(vault) = work.await {
                cx.update(|cx| {
                    cx.global_mut::<Self>().keychain_current = true;
                    Self::put(vault, cx);
                });
            }
        })
        .detach();
    }

    pub fn path(cx: &App) -> Option<PathBuf> {
        cx.global::<Self>().path.clone()
    }

    /// A vault file exists but has not been unlocked in this run.
    pub fn locked(cx: &App) -> bool {
        let k = cx.global::<Self>();
        k.open.is_none() && k.path.as_ref().is_some_and(|p| p.exists())
    }

    pub fn exists(cx: &App) -> bool {
        let k = cx.global::<Self>();
        k.open.is_some() || k.path.as_ref().is_some_and(|p| p.exists())
    }

    pub fn lookup(key: &Key, cx: &App) -> Option<SecretString> {
        cx.global::<Self>().open.as_ref()?.get(key).cloned()
    }

    /// Number of saved secrets, when the vault is open.
    pub fn len(cx: &App) -> Option<usize> {
        cx.global::<Self>().open.as_ref().map(Vault::len)
    }

    /// The saved entries, when the vault is open. Keys only, never values.
    pub fn keys(cx: &App) -> Option<Vec<Key>> {
        let v = cx.global::<Self>().open.as_ref()?;
        Some(v.keys().cloned().collect())
    }

    /// Forgets the unlocked vault, zeroing its secrets; the next use asks for the passphrase
    /// again. Open sessions are untouched: they hold no vault data.
    pub fn lock(cx: &mut App) {
        if let Some(mut vault) = cx.global_mut::<Self>().open.take() {
            vault.wipe();
        }
    }

    pub fn take(cx: &mut App) -> Option<Vault> {
        cx.global_mut::<Self>().open.take()
    }

    /// Hands an unlocked vault back. With the Keychain opt-in on, the passphrase is stored
    /// the first time (and again after it changes).
    pub fn put(vault: Vault, cx: &mut App) {
        let k = cx.global_mut::<Self>();
        if k.keychain && !k.keychain_current {
            match crate::keychain::store(vault.passphrase()) {
                Ok(()) => k.keychain_current = true,
                Err(e) => tracing::warn!(error = %e, "vault_keychain_store_failed"),
            }
        }
        k.open = Some(vault);
    }

    /// Changes the open vault in place; the caller then calls [`Keeper::persist`].
    pub fn edit<R>(cx: &mut App, change: impl FnOnce(&mut Vault) -> R) -> Option<R> {
        cx.global_mut::<Self>().open.as_mut().map(change)
    }

    /// The Keychain opt-in: stores the open vault's passphrase now, or removes the item.
    ///
    /// # Errors
    /// A message for the person when the vault is not open or the Keychain refuses.
    pub fn set_keychain(on: bool, cx: &mut App) -> Result<(), String> {
        let k = cx.global_mut::<Self>();
        if on {
            let Some(vault) = k.open.as_ref() else {
                return Err("Unlock the vault first: its passphrase is what gets stored.".into());
            };
            crate::keychain::store(vault.passphrase())?;
            k.keychain = true;
            k.keychain_current = true;
        } else {
            crate::keychain::delete()?;
            k.keychain = false;
            k.keychain_current = false;
        }
        Ok(())
    }

    /// The vault was re-sealed under a new passphrase: the Keychain item, if any, is stale.
    pub fn passphrase_changed(cx: &mut App) {
        cx.global_mut::<Self>().keychain_current = false;
    }

    /// Writes a copy of the open vault off the UI thread. Writes land in the order asked.
    pub fn persist(cx: &mut App) -> Option<Task<Result<(), String>>> {
        let k = cx.global_mut::<Self>();
        let path = k.path.clone()?;
        let snapshot = k.open.as_ref()?.snapshot();
        k.ticket += 1;
        let ticket = k.ticket;
        Some(cx.background_spawn(async move {
            in_order(ticket, || snapshot.save(&path))
                .unwrap_or(Ok(()))
                .map_err(|e| e.to_string())
        }))
    }

    /// Re-seals the vault file under a new passphrase off the UI thread, in order with other
    /// writes. Returns the vault, now open under the new passphrase.
    pub fn rekey(
        current: SecretString,
        new: SecretString,
        cx: &mut App,
    ) -> Option<Task<Result<Vault, VaultError>>> {
        let k = cx.global_mut::<Self>();
        let path = k.path.clone()?;
        k.ticket += 1;
        let ticket = k.ticket;
        Some(cx.background_spawn(async move {
            in_order(ticket, || Vault::change_passphrase(&path, current, new))
                .unwrap_or(Err(VaultError::WrongPassphrase))
        }))
    }
}

/// Whether a vault idle for `idle` should lock; `minutes` is the setting, 0 meaning never.
pub fn idle_expired(idle: Duration, minutes: u32) -> bool {
    minutes > 0 && idle >= Duration::from_secs(u64::from(minutes) * 60)
}

/// The idle-lock choice `delta` steps from `current` along the settings choices; stays at the
/// ends.
pub fn step_lock(current: u32, delta: isize) -> u32 {
    let choices = crate::settings::VAULT_LOCK_CHOICES;
    let at = choices
        .iter()
        .position(|&m| m == current)
        .unwrap_or(crate::settings::VAULT_LOCK_DEFAULT_INDEX);
    choices[at.saturating_add_signed(delta).min(choices.len() - 1)]
}

/// The vault entry that answers `prompt` on a connection to `host:port`; `None` for questions
/// a vault does not answer (host keys, keyboard-interactive).
pub fn key_for(prompt: &Prompt, host: &str, port: u16) -> Option<Key> {
    match prompt {
        Prompt::Password { user, .. } => Some(Key::Password {
            user: user.clone(),
            host: host.to_owned(),
            port,
        }),
        Prompt::KeyPassphrase { path, .. } => Some(Key::KeyPassphrase {
            path: path.display().to_string(),
        }),
        Prompt::Challenge { .. } | Prompt::UnknownHostKey { .. } => None,
    }
}

/// Answers a password or passphrase prompt without asking; any other prompt is returned.
pub fn answer(prompt: Prompt, secret: SecretString) -> Option<Prompt> {
    match prompt {
        Prompt::Password { reply, .. } | Prompt::KeyPassphrase { reply, .. } => {
            let _ = reply.send(Some(secret));
            None
        }
        other => Some(other),
    }
}

pub fn is_yes(answer: Option<&SecretString>) -> bool {
    answer.is_some_and(|a| matches!(a.expose_secret().trim(), "y" | "yes" | "Y" | "Yes"))
}

pub fn describe(key: &Key) -> String {
    match key {
        Key::Password { user, host, .. } => format!("the password for {user}@{host}"),
        Key::KeyPassphrase { path } => format!("the passphrase for {path}"),
        Key::SshKey { name } => format!("the key {name}"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tern_ssh::oneshot;

    #[test]
    fn password_key_uses_the_prompt_user_and_the_connection_address() {
        let (reply, _rx) = oneshot::channel();
        let prompt = Prompt::Password {
            user: "deploy".into(),
            host: "alias".into(),
            reply,
        };
        assert_eq!(
            key_for(&prompt, "10.0.0.5", 2222),
            Some(Key::Password {
                user: "deploy".into(),
                host: "10.0.0.5".into(),
                port: 2222
            })
        );
    }

    #[test]
    fn host_key_questions_are_never_answered_from_the_vault() {
        let (reply, _rx) = oneshot::channel();
        let prompt = Prompt::UnknownHostKey {
            host: "h".into(),
            port: 22,
            algorithm: "ed25519".into(),
            fingerprint_sha256: "SHA256:x".into(),
            reply,
        };
        assert_eq!(key_for(&prompt, "h", 22), None);
        assert!(answer(prompt, SecretString::from("x".to_owned())).is_some());
    }

    #[test]
    fn answering_sends_the_secret() {
        let (reply, mut rx) = oneshot::channel();
        let prompt = Prompt::Password {
            user: "u".into(),
            host: "h".into(),
            reply,
        };
        assert!(answer(prompt, SecretString::from("hunter2".to_owned())).is_none());
        assert_eq!(
            rx.try_recv().unwrap().unwrap().unwrap().expose_secret(),
            "hunter2"
        );
    }

    #[test]
    fn locks_only_after_the_whole_idle_period_and_never_when_off() {
        let m = |n: u64| Duration::from_secs(n * 60);
        assert!(!idle_expired(m(14), 15));
        assert!(idle_expired(m(15), 15));
        assert!(idle_expired(m(90), 5));
        assert!(!idle_expired(m(10_000), 0));
        assert!(!idle_expired(Duration::ZERO, 5));
    }

    #[test]
    fn the_lock_stepper_walks_the_choices_and_stops_at_the_ends() {
        assert_eq!(step_lock(15, 1), 30);
        assert_eq!(step_lock(15, -1), 5);
        assert_eq!(step_lock(5, -1), 0);
        assert_eq!(step_lock(0, -1), 0);
        assert_eq!(step_lock(60, 1), 60);
        // A value that is not a choice (a hand-edited file) starts from the default.
        assert_eq!(step_lock(7, 1), 30);
    }

    #[test]
    fn an_older_write_never_overwrites_a_newer_one() {
        // Tickets far above any a test run reaches, so the shared counter is not disturbed.
        let base = 1_000_000;
        assert_eq!(in_order(base + 2, || "newer"), Some("newer"));
        assert_eq!(in_order(base + 1, || "older"), None);
        assert_eq!(in_order(base + 2, || "same"), Some("same"));
        assert_eq!(in_order(base + 3, || "newest"), Some("newest"));
    }

    #[test]
    fn only_yes_saves() {
        let s = |t: &str| SecretString::from(t.to_owned());
        assert!(is_yes(Some(&s("yes"))));
        assert!(is_yes(Some(&s("y"))));
        assert!(!is_yes(Some(&s("no"))));
        assert!(!is_yes(Some(&s(""))));
        assert!(!is_yes(None));
    }
}
