//! Settings → Sync: keep tern's connections, settings, shortcuts and vault the same on every
//! Mac through a private GitHub gist (see `tern-sync`). The vault file travels as it is; the
//! rest is sealed with the vault passphrase first, so GitHub only ever holds ciphertext.

use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Subscription, div, px,
};
use serde::{Deserialize, Serialize};
use tern_ssh::SecretString;
use tern_sync::{Files, Gist, Plan, TokenSource};
use tern_vault::{Vault, VaultError};

use super::{Shell, ToastKind};
use crate::keeper::Keeper;
use crate::settings_widgets as w;
use crate::text_input::{InputColors, TextInput};

/// The files that travel besides the vault, by their name in the settings directory.
const SYNCED: [&str; 3] = ["hosts.json", "settings.json", "keymap.json"];
const VAULT_FILE: &str = "vault.age";
const STATE_FILE: &str = "sync.json";

#[derive(Default, Serialize, Deserialize)]
struct SyncState {
    /// The bundle hash both sides had after the last sync.
    last_hash: Option<String>,
}

pub(super) struct SyncUi {
    token_input: Entity<TextInput>,
    pass_input: Entity<TextInput>,
    token_source: Option<TokenSource>,
    busy: bool,
    /// The last outcome: ok?, text.
    message: Option<(bool, String)>,
    conflict: bool,
    _repaint: Vec<Subscription>,
}

enum Outcome {
    UpToDate,
    Pushed,
    Pulled,
    Conflict,
}

impl Shell {
    /// Builds the page state the first time Sync is opened and checks for a token off the UI
    /// thread (`gh auth token` is a process).
    pub(super) fn ensure_sync_ui(&mut self, cx: &mut Context<Self>) {
        if self.sync_ui.is_some() {
            return;
        }
        let t = self.theme;
        let colors = InputColors {
            text: t.text,
            placeholder: t.faint,
            cursor: t.accent,
            selection: t.accent.opacity(0.35),
        };
        let token_input =
            cx.new(|cx| TextInput::new("ghp_… with the gist scope", true, colors, cx));
        let pass_input = cx.new(|cx| TextInput::new("Vault passphrase", true, colors, cx));
        let repaint = vec![
            cx.observe(&token_input, |_, _, cx| cx.notify()),
            cx.observe(&pass_input, |_, _, cx| cx.notify()),
        ];
        self.sync_ui = Some(SyncUi {
            token_input,
            pass_input,
            token_source: None,
            busy: false,
            message: None,
            conflict: false,
            _repaint: repaint,
        });
        self.refresh_token_source(cx);
    }

    fn refresh_token_source(&mut self, cx: &mut Context<Self>) {
        let work = cx.background_spawn(async { tern_sync::token().ok().map(|(_, s)| s) });
        cx.spawn(async move |this, cx| {
            let source = work.await;
            let _ = this.update(cx, |s, cx| {
                if let Some(ui) = s.sync_ui.as_mut() {
                    ui.token_source = source;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn save_token(&mut self, cx: &mut Context<Self>) {
        let Some(ui) = self.sync_ui.as_mut() else {
            return;
        };
        let token = ui.token_input.read(cx).text().to_owned();
        let saved = tern_sync::save_token(&token);
        ui.token_input.update(cx, |i, cx| i.set_text("", cx));
        match saved {
            Ok(()) => self.notify_toast(ToastKind::Positive, "GitHub token saved", cx),
            Err(e) => self.notify_toast(ToastKind::Critical, e.to_string(), cx),
        }
        self.refresh_token_source(cx);
    }

    fn forget_token(&mut self, cx: &mut Context<Self>) {
        match tern_sync::forget_token() {
            Ok(()) => self.notify_toast(ToastKind::Default, "GitHub token removed", cx),
            Err(e) => self.notify_toast(ToastKind::Critical, e.to_string(), cx),
        }
        self.refresh_token_source(cx);
    }

    /// Syncs; `force` settles a conflict (Push keeps this Mac, Pull takes GitHub's copy).
    fn sync_now(&mut self, force: Option<Plan>, cx: &mut Context<Self>) {
        let Some(dir) = crate::settings::dir() else {
            return;
        };
        let Some(ui) = self.sync_ui.as_mut() else {
            return;
        };
        if ui.busy {
            return;
        }
        let passphrase = Some(ui.pass_input.read(cx).text().to_owned()).filter(|p| !p.is_empty());
        let open = Keeper::take(cx);
        if open.is_none() && passphrase.is_none() {
            self.notify_toast(
                ToastKind::Critical,
                "Enter the vault passphrase first: it encrypts what goes to GitHub",
                cx,
            );
            return;
        }
        ui.busy = true;
        ui.message = None;
        ui.conflict = false;
        cx.notify();
        let work =
            cx.background_spawn(async move { run(&dir, open, passphrase.as_deref(), force) });
        cx.spawn(async move |this, cx| {
            let (vault, result) = work.await;
            let _ = this.update_in(cx, |s, window, cx| {
                if let Some(v) = vault {
                    Keeper::put(v, cx);
                }
                // The passphrase field may now be hidden; a hidden field must not keep focus,
                // or Escape and every shortcut go nowhere.
                window.focus(&s.focus, cx);
                let pulled = matches!(result, Ok(Outcome::Pulled));
                if let Some(ui) = s.sync_ui.as_mut() {
                    ui.busy = false;
                    // A conflict needs a choice, so it stays on the page; the rest is feedback.
                    ui.message = match &result {
                        Ok(Outcome::Conflict) => Some((
                            false,
                            "This Mac and GitHub both changed since the last sync. Choose which \
                             one to keep."
                                .into(),
                        )),
                        _ => None,
                    };
                    ui.conflict = matches!(result, Ok(Outcome::Conflict));
                    if result.is_ok() {
                        ui.pass_input.update(cx, |i, cx| i.set_text("", cx));
                    }
                }
                match &result {
                    Ok(Outcome::UpToDate) => {
                        s.notify_toast(ToastKind::Default, "Already up to date", cx)
                    }
                    Ok(Outcome::Pushed) => {
                        s.notify_toast(ToastKind::Positive, "Uploaded to GitHub", cx)
                    }
                    Ok(Outcome::Pulled) => {
                        s.notify_toast(ToastKind::Positive, "Downloaded from GitHub", cx)
                    }
                    Ok(Outcome::Conflict) => {}
                    Err(e) => s.notify_toast(ToastKind::Critical, e.clone(), cx),
                }
                if pulled {
                    s.reload_from_disk(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// After a pull: settings, shortcuts and connections are re-read and applied.
    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = crate::settings::dir() else {
            return;
        };
        self.settings = crate::settings::Settings::load(&dir);
        cx.set_reduce_motion(self.settings.reduce_motion);
        cx.set_global(crate::keymap::Keymap::load(&dir));
        crate::keymap::apply(cx);
        match crate::connections::load(&dir) {
            Ok(list) => {
                self.connections = list;
                self.store_error = None;
            }
            Err(e) => self.store_error = Some(e.to_string()),
        }
        self.refresh_hosts();
        self.restyle_tabs(cx);
        self.apply_option_as_meta(cx);
    }

    pub(super) fn sync_page(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let Some(ui) = &self.sync_ui else {
            return w::page_column().child(w::page_header(&t, "Sync"));
        };
        let account = match ui.token_source {
            Some(TokenSource::GhCli) => "Using the GitHub CLI's sign-in (gh)",
            Some(TokenSource::Keychain) => "Token saved in the macOS Keychain",
            None => "Not connected",
        };
        let mut github = w::card(&t).child(w::row(
            &t,
            true,
            "GitHub",
            Some(account.into()),
            match ui.token_source {
                Some(TokenSource::Keychain) => w::button(&t, "sync-forget", "Forget token")
                    .on_click(cx.listener(|s, _, _, cx| s.forget_token(cx)))
                    .into_any_element(),
                _ => div().into_any_element(),
            },
        ));
        if ui.token_source != Some(TokenSource::Keychain) {
            github = github.child(w::row(
                &t,
                false,
                "Personal access token",
                Some("Optional when gh is signed in. Needs only the gist scope.".into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(field(&ui.token_input, &t))
                    .child(
                        w::button(&t, "sync-save-token", "Save")
                            .on_click(cx.listener(|s, _, _, cx| s.save_token(cx))),
                    ),
            ));
        }
        let needs_pass = Keeper::len(cx).is_none();
        let mut sync = w::card(&t);
        if needs_pass {
            sync = sync.child(w::row(
                &t,
                true,
                "Vault passphrase",
                Some(if Keeper::exists(cx) {
                    "Unlocks this Mac's vault for the sync".into()
                } else {
                    "No vault here yet: GitHub's passphrase, or a new one to start".into()
                }),
                field(&ui.pass_input, &t),
            ));
        }
        let label = if ui.busy { "Syncing…" } else { "Sync now" };
        sync = sync.child(w::row(
            &t,
            !needs_pass,
            "Sync now",
            Some(
                "Connections, settings, shortcuts and the vault, encrypted, in a private gist"
                    .into(),
            ),
            w::button(&t, "sync-now", label)
                .on_click(cx.listener(|s, _, _, cx| s.sync_now(None, cx))),
        ));
        if ui.conflict {
            sync = sync.child(w::row(
                &t,
                false,
                "Resolve",
                Some(
                    "Keep this Mac uploads it over GitHub's; Use GitHub replaces this Mac's".into(),
                ),
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        w::button(&t, "sync-keep-mac", "Keep this Mac")
                            .on_click(cx.listener(|s, _, _, cx| s.sync_now(Some(Plan::Push), cx))),
                    )
                    .child(
                        w::button(&t, "sync-use-github", "Use GitHub")
                            .on_click(cx.listener(|s, _, _, cx| s.sync_now(Some(Plan::Pull), cx))),
                    ),
            ));
        }
        w::page_column()
            .child(w::page_header(&t, "Sync"))
            .child(w::page_subtitle(
                &t,
                "One private gist on your GitHub account. GitHub stores only encrypted data.",
            ))
            .when_some(ui.message.clone(), |el, (ok, text)| {
                el.child(
                    w::page_subtitle(&t, SharedString::from(text)).text_color(if ok {
                        t.muted
                    } else {
                        t.danger
                    }),
                )
            })
            .child(w::section(&t, "Account", github))
            .child(w::section(&t, "Sync", sync))
    }
}

fn field(input: &Entity<TextInput>, t: &crate::theme::Theme) -> impl IntoElement + use<> {
    div()
        .w(px(240.))
        .h(px(30.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .border_1()
        .border_color(t.border)
        .bg(t.row_hover)
        .text_sm()
        .child(input.clone())
}

/// The whole sync, off the UI thread. Returns the vault to hand back to the keeper (the
/// downloaded one after a pull) and what happened.
fn run(
    dir: &Path,
    open: Option<Vault>,
    passphrase: Option<&str>,
    force: Option<Plan>,
) -> (Option<Vault>, Result<Outcome, String>) {
    let vault_path = dir.join(VAULT_FILE);
    let vault = match (open, passphrase) {
        (Some(v), _) => v,
        (None, Some(p)) if vault_path.exists() => {
            match Vault::unlock(&vault_path, SecretString::from(p.to_owned())) {
                Ok(v) => v,
                Err(VaultError::WrongPassphrase) => {
                    return (None, Err("Wrong vault passphrase.".into()));
                }
                Err(e) => return (None, Err(e.to_string())),
            }
        }
        (None, Some(p)) => Vault::new(SecretString::from(p.to_owned())),
        (None, None) => return (None, Err("The vault is locked.".into())),
    };
    let result = sync(dir, &vault, passphrase, force);
    match result {
        Ok((outcome, Some(pulled))) => (Some(pulled), Ok(outcome)),
        Ok((outcome, None)) => (Some(vault), Ok(outcome)),
        Err(e) => (Some(vault), Err(e)),
    }
}

/// Returns the outcome, and the downloaded vault after a pull.
fn sync(
    dir: &Path,
    vault: &Vault,
    passphrase: Option<&str>,
    force: Option<Plan>,
) -> Result<(Outcome, Option<Vault>), String> {
    let vault_path = dir.join(VAULT_FILE);
    if !vault_path.exists() {
        vault.save(&vault_path).map_err(|e| e.to_string())?;
    }
    let local = read_local(dir)?;
    let local_hash = tern_sync::hash(&local);
    let state: SyncState = std::fs::read_to_string(dir.join(STATE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let (token, _) = tern_sync::token().map_err(|e| e.to_string())?;
    let gist = Gist::new(token);
    let remote = gist.fetch().map_err(|e| e.to_string())?;
    // A Mac that never synced and holds nothing of its own (no connections, an empty vault)
    // just takes GitHub's copy instead of asking which side wins.
    let fresh = state.last_hash.is_none() && !local.contains_key("hosts.json") && vault.is_empty();
    let plan = force.unwrap_or_else(|| match (&remote, fresh) {
        (Some(_), true) => Plan::Pull,
        _ => tern_sync::decide(
            &local_hash,
            remote.as_ref().map(|r| r.hash.as_str()),
            state.last_hash.as_deref(),
        ),
    });
    match plan {
        Plan::UpToDate => {
            write_state(dir, &local_hash)?;
            Ok((Outcome::UpToDate, None))
        }
        Plan::Conflict => Ok((Outcome::Conflict, None)),
        Plan::Push => {
            let mut rest = local.clone();
            let vault_bytes = rest.remove(VAULT_FILE).unwrap_or_default();
            let sealed = vault.seal(&encode(&rest)?).map_err(|e| e.to_string())?;
            let device = std::env::var("USER").unwrap_or_else(|_| "mac".into());
            gist.push(&local_hash, &device, &vault_bytes, &sealed)
                .map_err(|e| e.to_string())?;
            write_state(dir, &local_hash)?;
            Ok((Outcome::Pushed, None))
        }
        Plan::Pull => {
            let Some(remote) = remote else {
                return Err("GitHub has no tern-sync gist yet.".into());
            };
            // GitHub's vault may use another passphrase than this Mac's: a typed one wins.
            let theirs = match passphrase {
                Some(p) => Vault::unlock_bytes(&remote.vault, SecretString::from(p.to_owned())),
                None => vault.reopen(&remote.vault),
            }
            .map_err(|e| match e {
                VaultError::WrongPassphrase => {
                    "GitHub's vault uses another passphrase: enter it above and sync again."
                        .to_owned()
                }
                other => other.to_string(),
            })?;
            let rest = decode(&theirs.open(&remote.sealed).map_err(|e| e.to_string())?)?;
            for (name, bytes) in &rest {
                write_atomic(&dir.join(name), bytes)?;
            }
            for name in SYNCED {
                if !rest.contains_key(name) {
                    let _ = std::fs::remove_file(dir.join(name));
                }
            }
            write_atomic(&vault_path, &remote.vault)?;
            write_state(dir, &remote.hash)?;
            Ok((Outcome::Pulled, Some(theirs)))
        }
    }
}

fn read_local(dir: &Path) -> Result<Files, String> {
    let mut files = Files::new();
    for name in SYNCED.into_iter().chain([VAULT_FILE]) {
        match std::fs::read(dir.join(name)) {
            Ok(bytes) => {
                files.insert(name.to_owned(), bytes);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{name}: {e}")),
        }
    }
    Ok(files)
}

fn encode(files: &Files) -> Result<Vec<u8>, String> {
    let map: BTreeMap<&str, String> = files
        .iter()
        .map(|(n, b)| (n.as_str(), B64.encode(b)))
        .collect();
    serde_json::to_vec(&map).map_err(|e| e.to_string())
}

/// The inverse of [`encode`]; names outside [`SYNCED`] are dropped, so a tampered bundle
/// cannot write elsewhere.
fn decode(bytes: &[u8]) -> Result<Files, String> {
    let map: BTreeMap<String, String> =
        serde_json::from_slice(bytes).map_err(|e| format!("sync data: {e}"))?;
    map.into_iter()
        .filter(|(n, _)| SYNCED.contains(&n.as_str()))
        .map(|(n, b)| {
            B64.decode(b)
                .map(|bytes| (n.clone(), bytes))
                .map_err(|e| format!("{n}: {e}"))
        })
        .collect()
}

fn write_state(dir: &Path, hash: &str) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(&SyncState {
        last_hash: Some(hash.to_owned()),
    })
    .map_err(|e| e.to_string())?;
    write_atomic(&dir.join(STATE_FILE), &json)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("sync.tmp");
    std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trips_and_drops_unknown_names() {
        let mut files = Files::new();
        files.insert("hosts.json".into(), b"{\"v\":1}".to_vec());
        files.insert("settings.json".into(), vec![0, 255, 7]);
        assert_eq!(decode(&encode(&files).unwrap()).unwrap(), files);

        let evil = br#"{"../../.ssh/authorized_keys":"aGk=","hosts.json":"e30="}"#;
        let out = decode(evil).unwrap();
        assert_eq!(out.keys().collect::<Vec<_>>(), vec!["hosts.json"]);
    }
}
