//! The app's one vault: where it lives and, once unlocked, the open [`Vault`] every tab reads.
//! Unlocking and saving run scrypt (about a second), so both happen off the UI thread.

use std::path::PathBuf;

use gpui::{App, Global};
use tern_ssh::{ExposeSecret, Prompt, SecretString};
use tern_vault::{Key, Vault};

pub struct Keeper {
    path: Option<PathBuf>,
    open: Option<Vault>,
}

impl Global for Keeper {}

impl Keeper {
    pub fn install(cx: &mut App) {
        let path = crate::settings::dir().map(|d| d.join("vault.age"));
        cx.set_global(Self { path, open: None });
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

    /// Forgets the unlocked vault; the next use asks for the passphrase again.
    pub fn lock(cx: &mut App) {
        cx.global_mut::<Self>().open = None;
    }

    pub fn take(cx: &mut App) -> Option<Vault> {
        cx.global_mut::<Self>().open.take()
    }

    pub fn put(vault: Vault, cx: &mut App) {
        cx.global_mut::<Self>().open = Some(vault);
    }
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
    fn only_yes_saves() {
        let s = |t: &str| SecretString::from(t.to_owned());
        assert!(is_yes(Some(&s("yes"))));
        assert!(is_yes(Some(&s("y"))));
        assert!(!is_yes(Some(&s("no"))));
        assert!(!is_yes(Some(&s(""))));
        assert!(!is_yes(None));
    }
}
