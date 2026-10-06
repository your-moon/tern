// Adapted from russh russh/examples/client_exec_interactive.rs and russh/src/client/mod.rs
// (Apache-2.0) for the publickey / keyboard-interactive / password calls, and from
// CrabPort crabport-ssh/src/backend.rs (Apache-2.0) for the overall auth ordering.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::channel::oneshot;
use russh::client::{self, AuthResult, KeyboardInteractiveAuthResponse};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::AgentClient;
use russh::keys::{PrivateKeyWithHashAlg, load_secret_key};
use russh::{MethodKind, MethodSet};
use secrecy::{ExposeSecret, SecretString};

use crate::error::Failure;
use crate::hostkey::Handler;
use crate::{ChallengePrompt, MemoryKey, Prompt, SessionEvent};

const MAX_TRIES: usize = 3;

pub(crate) struct Authenticator<'a> {
    pub session: &'a mut client::Handle<Handler>,
    pub user: &'a str,
    pub host: &'a str,
    pub identity_files: Vec<PathBuf>,
    pub events: &'a async_channel::Sender<SessionEvent>,
    memory_keys: Vec<MemoryKey>,
    remaining: MethodSet,
    /// The last failure said one step of a multi-step login passed (`partial_success`).
    progressed: bool,
}

impl<'a> Authenticator<'a> {
    pub fn new(
        session: &'a mut client::Handle<Handler>,
        user: &'a str,
        host: &'a str,
        identity_files: Vec<PathBuf>,
        events: &'a async_channel::Sender<SessionEvent>,
    ) -> Self {
        Self {
            session,
            user,
            host,
            identity_files,
            events,
            memory_keys: Vec::new(),
            remaining: MethodSet::client_supported(),
            progressed: false,
        }
    }

    /// Keys held in memory (the vault's), tried after the agent and before key files.
    pub fn with_memory_keys(mut self, keys: Vec<MemoryKey>) -> Self {
        self.memory_keys = keys;
        self
    }

    fn allows(&self, m: MethodKind) -> bool {
        self.remaining.contains(&m)
    }

    /// Folds an auth result into server-advertised state. Returns true on success.
    fn absorb(&mut self, r: AuthResult) -> bool {
        match r {
            AuthResult::Success => true,
            AuthResult::Failure {
                remaining_methods,
                partial_success,
            } => {
                self.remaining = remaining_methods;
                self.progressed = partial_success;
                false
            }
        }
    }

    pub async fn run(&mut self) -> Result<(), Failure> {
        let none = self.session.authenticate_none(self.user).await?;
        if self.absorb(none) {
            tracing::info!(host = %self.host, auth_method = "none", "ssh_authenticated");
            return Ok(());
        }
        if self.allows(MethodKind::PublicKey) {
            if self.try_agent().await? {
                tracing::info!(host = %self.host, auth_method = "agent", "ssh_authenticated");
                return Ok(());
            }
            if self.try_memory_keys().await? {
                tracing::info!(host = %self.host, auth_method = "vault_key", "ssh_authenticated");
                return Ok(());
            }
            if self.try_identity_files().await? {
                tracing::info!(host = %self.host, auth_method = "publickey", "ssh_authenticated");
                return Ok(());
            }
        }
        if self.try_interactive_or_password().await? {
            return Ok(());
        }
        Err(Failure::AuthFailed)
    }

    async fn try_agent(&mut self) -> Result<bool, Failure> {
        let mut agent = match AgentClient::connect_env().await {
            Ok(a) => a,
            Err(e) => {
                tracing::debug!(error = %e, "ssh_agent_unavailable");
                return Ok(false);
            }
        };
        let identities = match agent.request_identities().await {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(error = %e, "ssh_agent_list_failed");
                return Ok(false);
            }
        };
        tracing::debug!(identity_count = identities.len(), "ssh_agent_identities");
        let rsa_hash = self.session.best_supported_rsa_hash().await?.flatten();
        for id in identities {
            // Certificates in the agent are not supported yet; plain keys only.
            let AgentIdentity::PublicKey { key, .. } = id else {
                continue;
            };
            if !self.allows(MethodKind::PublicKey) {
                break;
            }
            let hash = if key.algorithm().is_rsa() {
                rsa_hash
            } else {
                None
            };
            match self
                .session
                .authenticate_publickey_with(self.user, key, hash, &mut agent)
                .await
            {
                Ok(r) => {
                    if self.absorb(r) {
                        return Ok(true);
                    }
                }
                Err(e) => tracing::warn!(error = %e, "ssh_agent_sign_failed"),
            }
        }
        Ok(false)
    }

    /// Offers each in-memory key; an unreadable one is skipped, never printed.
    async fn try_memory_keys(&mut self) -> Result<bool, Failure> {
        if self.memory_keys.is_empty() {
            return Ok(false);
        }
        let rsa_hash = self.session.best_supported_rsa_hash().await?.flatten();
        for mk in self.memory_keys.clone() {
            if !self.allows(MethodKind::PublicKey) {
                break;
            }
            let key = match russh::keys::decode_secret_key(mk.openssh.expose_secret(), None) {
                Ok(k) => k,
                Err(e) => {
                    tracing::warn!(key = %mk.name, error = %e, "ssh_vault_key_unreadable");
                    continue;
                }
            };
            let hash = if key.algorithm().is_rsa() {
                rsa_hash
            } else {
                None
            };
            let r = self
                .session
                .authenticate_publickey(self.user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await?;
            if self.absorb(r) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn try_identity_files(&mut self) -> Result<bool, Failure> {
        let files = if self.identity_files.is_empty() {
            crate::config::default_identity_files()
        } else {
            self.identity_files.clone()
        };
        let rsa_hash = self.session.best_supported_rsa_hash().await?.flatten();
        for path in files {
            if !self.allows(MethodKind::PublicKey) {
                break;
            }
            let Some(key) = self.load_key(&path).await? else {
                continue;
            };
            let hash = if key.algorithm().is_rsa() {
                rsa_hash
            } else {
                None
            };
            let r = self
                .session
                .authenticate_publickey(self.user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await?;
            if self.absorb(r) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Loads a private key, asking for its passphrase (up to 3 times) if it is encrypted.
    /// `None` means "skip this key".
    async fn load_key(&self, path: &Path) -> Result<Option<russh::keys::PrivateKey>, Failure> {
        match load_secret_key(path, None) {
            Ok(k) => return Ok(Some(k)),
            Err(russh::keys::Error::KeyIsEncrypted) => {}
            Err(russh::keys::Error::IO(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(path = %path.display(), "ssh_identity_missing");
                return Ok(None);
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "ssh_identity_unreadable");
                return Ok(None);
            }
        }
        for _ in 0..MAX_TRIES {
            let reply = ask(self.events, |reply| Prompt::KeyPassphrase {
                path: path.to_path_buf(),
                reply,
            })
            .await?;
            let Some(pass) = reply else {
                return Ok(None);
            };
            match load_secret_key(path, Some(pass.expose_secret())) {
                Ok(k) => return Ok(Some(k)),
                Err(_) => tracing::debug!(path = %path.display(), "ssh_passphrase_rejected"),
            }
        }
        Ok(None)
    }

    async fn try_interactive_or_password(&mut self) -> Result<bool, Failure> {
        let mut kbd = self.allows(MethodKind::KeyboardInteractive);
        let mut tries = 0;
        while tries < MAX_TRIES {
            if kbd {
                match self.keyboard_interactive().await? {
                    Kbd::Success => {
                        tracing::info!(host = %self.host, auth_method = "keyboard_interactive", "ssh_authenticated");
                        return Ok(true);
                    }
                    Kbd::Rejected { prompted } => {
                        if !prompted || !self.allows(MethodKind::KeyboardInteractive) {
                            kbd = false;
                        } else {
                            tries += 1;
                        }
                    }
                }
            } else if self.allows(MethodKind::Password) {
                let Some(pass) = self.ask_password().await? else {
                    return Err(Failure::AuthCancelled);
                };
                let r = self
                    .session
                    .authenticate_password(self.user, pass.expose_secret())
                    .await?;
                if self.absorb(r) {
                    tracing::info!(host = %self.host, auth_method = "password", "ssh_authenticated");
                    return Ok(true);
                }
                if self.progressed {
                    // The password was right and the server wants another step (a code, as
                    // `AuthenticationMethods password,keyboard-interactive` does): that is not a
                    // wrong try, and the next step is the one it now offers.
                    kbd = self.allows(MethodKind::KeyboardInteractive);
                } else {
                    tries += 1;
                }
            } else {
                break;
            }
        }
        Ok(false)
    }

    async fn keyboard_interactive(&mut self) -> Result<Kbd, Failure> {
        let mut prompted = false;
        let mut resp = self
            .session
            .authenticate_keyboard_interactive_start(self.user, None)
            .await?;
        loop {
            match resp {
                KeyboardInteractiveAuthResponse::Success => return Ok(Kbd::Success),
                KeyboardInteractiveAuthResponse::Failure {
                    remaining_methods,
                    partial_success,
                } => {
                    self.remaining = remaining_methods;
                    self.progressed = partial_success;
                    return Ok(Kbd::Rejected { prompted });
                }
                KeyboardInteractiveAuthResponse::InfoRequest {
                    name,
                    instructions,
                    prompts,
                } => {
                    // A round with no prompts still needs an (empty) response.
                    let answers = if prompts.is_empty() {
                        Vec::new()
                    } else {
                        prompted = true;
                        let Some(answers) = self.ask_challenge(name, instructions, prompts).await?
                        else {
                            return Err(Failure::AuthCancelled);
                        };
                        answers
                    };
                    resp = self
                        .session
                        .authenticate_keyboard_interactive_respond(answers)
                        .await?;
                }
            }
        }
    }

    async fn ask_challenge(
        &self,
        name: String,
        instructions: String,
        prompts: Vec<russh::client::Prompt>,
    ) -> Result<Option<Vec<String>>, Failure> {
        let expected = prompts.len();
        let prompts = prompts
            .into_iter()
            .map(|p| ChallengePrompt {
                text: p.prompt,
                echo: p.echo,
            })
            .collect();
        let (tx, rx) = oneshot::channel();
        self.events
            .send(SessionEvent::Prompt(Prompt::Challenge {
                name,
                instructions,
                prompts,
                reply: tx,
            }))
            .await
            .map_err(|_| Failure::UiGone)?;
        Ok(rx
            .await
            .ok()
            .flatten()
            .filter(|a| a.len() == expected)
            .map(|a| a.iter().map(|s| s.expose_secret().to_string()).collect()))
    }

    async fn ask_password(&self) -> Result<Option<SecretString>, Failure> {
        ask(self.events, |reply| Prompt::Password {
            user: self.user.to_string(),
            host: self.host.to_string(),
            reply,
        })
        .await
    }
}

enum Kbd {
    Success,
    Rejected { prompted: bool },
}

/// Sends a prompt to the UI and awaits its reply. A dropped reply sender counts as "cancelled".
async fn ask(
    events: &async_channel::Sender<SessionEvent>,
    make: impl FnOnce(oneshot::Sender<Option<SecretString>>) -> Prompt,
) -> Result<Option<SecretString>, Failure> {
    let (tx, rx) = oneshot::channel();
    events
        .send(SessionEvent::Prompt(make(tx)))
        .await
        .map_err(|_| Failure::UiGone)?;
    Ok(rx.await.unwrap_or(None))
}
