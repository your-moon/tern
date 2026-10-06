//! The vault side of logging in: answer prompts from the vault, unlock it when it is locked,
//! and offer to save what the user typed once the login worked.

use std::time::{Duration, Instant};

use gpui::{AppContext, Context};
use tern_ssh::{ExposeSecret, Prompt, SecretString};
use tern_vault::{Key, Vault, VaultError};

use super::Session;
use crate::keeper::{self, Keeper};
use crate::login::Login;

const QUIET: Duration = Duration::from_millis(500);
const QUIET_MAX: Duration = Duration::from_secs(3);

/// A question tern is asking for itself, and what to do with the answer.
pub(super) enum Flow {
    /// "Vault passphrase (Enter to skip)": then answer `prompt` from the vault or ask it.
    UnlockForPrompt(Prompt),
    /// "Save … in tern's vault? (yes/no)".
    OfferSave(Key, SecretString),
    UnlockToSave(Key, SecretString),
    NewPassphrase(Key, SecretString),
    RepeatPassphrase(Key, SecretString, SecretString),
}

impl Session {
    pub(super) fn on_prompt(&mut self, prompt: Prompt, cx: &mut Context<Self>) {
        let key = keeper::key_for(&prompt, &self.spec.host, self.spec.port);
        // Each secret is tried from the vault once per connection, so a stale one falls
        // through to asking instead of failing in a loop.
        if let Some(key) = key.filter(|k| !self.tried.contains(k)) {
            if let Some(secret) = Keeper::lookup(&key, cx) {
                self.tried.push(key.clone());
                self.show(
                    format!(
                        "\x1b[2m[{} from the vault]\x1b[0m\r\n",
                        keeper::describe(&key)
                    )
                    .as_bytes(),
                    cx,
                );
                if let Some(other) = keeper::answer(prompt, secret) {
                    self.ask_user(other, cx);
                }
                return;
            }
            if Keeper::locked(cx) {
                return self.ask_local(
                    "Vault passphrase (Enter to skip): ",
                    false,
                    Flow::UnlockForPrompt(prompt),
                    cx,
                );
            }
        }
        self.ask_user(prompt, cx);
    }

    /// The server's question, asked in the terminal; a typed password is remembered so it can
    /// be offered for saving once the login succeeds.
    fn ask_user(&mut self, prompt: Prompt, cx: &mut Context<Self>) {
        self.asking = keeper::key_for(&prompt, &self.spec.host, self.spec.port);
        // Asked again for the same secret means the last answer was refused; say so as
        // OpenSSH does, or a retry looks like the same prompt shown twice.
        if let Some(key) = &self.asking {
            if self.asked.contains(key) {
                self.show(b"Permission denied, please try again.\r\n", cx);
            } else {
                self.asked.push(key.clone());
            }
        }
        let (login, question) = Login::start(prompt);
        self.login = Some(login);
        self.show(&question, cx);
    }

    fn ask_local(&mut self, text: &str, echo: bool, flow: Flow, cx: &mut Context<Self>) {
        let (login, question) = Login::ask(text, echo);
        self.login = Some(login);
        self.flow = Some(flow);
        self.show(&question, cx);
    }

    /// Asks to save a typed secret once the remote has been quiet for [`QUIET`] (at most
    /// [`QUIET_MAX`] after login), so the question does not land inside the login banner.
    pub(super) fn offer_save(&mut self, cx: &mut Context<Self>) {
        if self.typed.is_none() {
            return;
        }
        let started = Instant::now();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(QUIET / 2).await;
                let quiet = this
                    .update(cx, |s, _| s.last_output.elapsed() >= QUIET)
                    .unwrap_or(true);
                if quiet || started.elapsed() >= QUIET_MAX {
                    break;
                }
            }
            let _ = this.update(cx, |s, cx| {
                if s.login.is_some() || s.status != super::Status::Connected {
                    return;
                }
                if let Some((key, secret)) = s.typed.take() {
                    let lead = if s.ends_line { "" } else { "\r\n" };
                    let text = format!(
                        "{lead}Save {} in tern's vault? (yes/no) ",
                        keeper::describe(&key)
                    );
                    s.ask_local(&text, true, Flow::OfferSave(key, secret), cx);
                }
            });
        })
        .detach();
    }

    /// A question finished; `answer` is what was typed, `None` after Ctrl-C.
    pub(super) fn answered(&mut self, answer: Option<SecretString>, cx: &mut Context<Self>) {
        let Some(flow) = self.flow.take() else {
            if let (Some(key), Some(secret)) = (self.asking.take(), answer) {
                self.typed = Some((key, secret));
            }
            return;
        };
        let typed = answer.filter(|a| !a.expose_secret().is_empty());
        match flow {
            Flow::UnlockForPrompt(prompt) => match typed {
                Some(passphrase) => self.unlock(passphrase, Some(prompt), None, cx),
                None => self.ask_user(prompt, cx),
            },
            Flow::OfferSave(key, secret) => {
                if !keeper::is_yes(typed.as_ref()) {
                    return;
                }
                if !Keeper::exists(cx) {
                    self.ask_local(
                        "New vault passphrase: ",
                        false,
                        Flow::NewPassphrase(key, secret),
                        cx,
                    );
                } else if Keeper::locked(cx) {
                    self.ask_local(
                        "Vault passphrase: ",
                        false,
                        Flow::UnlockToSave(key, secret),
                        cx,
                    );
                } else if let Some(mut vault) = Keeper::take(cx) {
                    vault.set(key, secret);
                    self.save(vault, cx);
                }
            }
            Flow::UnlockToSave(key, secret) => match typed {
                Some(passphrase) => self.unlock(passphrase, None, Some((key, secret)), cx),
                None => self.note("Not saved.", cx),
            },
            Flow::NewPassphrase(key, secret) => match typed {
                Some(first) => self.ask_local(
                    "Repeat vault passphrase: ",
                    false,
                    Flow::RepeatPassphrase(key, secret, first),
                    cx,
                ),
                None => self.note("Not saved.", cx),
            },
            Flow::RepeatPassphrase(key, secret, first) => {
                if typed.is_some_and(|again| again.expose_secret() == first.expose_secret()) {
                    let mut vault = Vault::new(first);
                    vault.set(key, secret);
                    self.save(vault, cx);
                } else {
                    self.note("Passphrases differ; not saved.", cx);
                }
            }
        }
    }

    /// Unlocks off the UI thread, then either answers `prompt` or saves `entry`.
    fn unlock(
        &mut self,
        passphrase: SecretString,
        prompt: Option<Prompt>,
        entry: Option<(Key, SecretString)>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = Keeper::path(cx) else {
            return;
        };
        self.note("Unlocking the vault…", cx);
        let work = cx.background_spawn(async move { Vault::unlock(&path, passphrase) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |s, cx| {
                let unlocked = result.is_ok();
                match result {
                    Ok(mut vault) => match entry {
                        Some((key, secret)) => {
                            vault.set(key, secret);
                            s.save(vault, cx);
                        }
                        None => Keeper::put(vault, cx),
                    },
                    Err(VaultError::WrongPassphrase) => s.note("Wrong vault passphrase.", cx),
                    Err(e) => s.note(&format!("Vault unavailable: {e}"), cx),
                }
                // A failed unlock goes straight to the server's question rather than asking
                // for the vault passphrase again.
                match prompt {
                    Some(prompt) if unlocked => s.on_prompt(prompt, cx),
                    Some(prompt) => s.ask_user(prompt, cx),
                    None => {}
                }
            });
        })
        .detach();
    }

    /// Encrypts and writes off the UI thread; the vault returns to the keeper afterwards.
    fn save(&mut self, vault: Vault, cx: &mut Context<Self>) {
        let Some(path) = Keeper::path(cx) else {
            return;
        };
        let work = cx.background_spawn(async move {
            let result = vault.save(&path);
            (vault, result)
        });
        cx.spawn(async move |this, cx| {
            let (vault, result) = work.await;
            let _ = this.update(cx, |s, cx| {
                Keeper::put(vault, cx);
                match result {
                    Ok(()) => s.note("Saved in the vault.", cx),
                    Err(e) => {
                        tracing::warn!(error = %e, "vault_save_failed");
                        s.note(&format!("Could not save the vault: {e}"), cx);
                    }
                }
            });
        })
        .detach();
    }

    fn note(&self, text: &str, cx: &mut Context<Self>) {
        self.show(format!("\x1b[2m{text}\x1b[0m\r\n").as_bytes(), cx);
    }
}
