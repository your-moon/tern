//! The gist side: find or create the private `tern-sync` gist, read and write its files.
//! Blocking HTTP (ureq with the system TLS); callers run it off the UI thread.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const API: &str = "https://api.github.com";
const DESCRIPTION: &str = "tern-sync";
const META: &str = "tern-meta.json";
const VAULT: &str = "vault.age.b64";
const DATA: &str = "data.age.b64";
const KEYCHAIN_SERVICE: &str = "tern";
const KEYCHAIN_ACCOUNT: &str = "github-token";

#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    #[error("no GitHub token: paste one in Settings → Sync, or sign in with `gh auth login`")]
    NoToken,
    #[error("GitHub refused the token (it needs the gist scope)")]
    Unauthorized,
    #[error("GitHub: {0}")]
    Http(String),
    #[error("the tern-sync gist is not in the expected format: {0}")]
    Format(String),
    #[error("Keychain: {0}")]
    Keychain(String),
}

impl From<ureq::Error> for GithubError {
    fn from(e: ureq::Error) -> Self {
        match e {
            ureq::Error::StatusCode(401 | 403) => GithubError::Unauthorized,
            other => GithubError::Http(other.to_string()),
        }
    }
}

/// Where the token came from, for the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Keychain,
    GhCli,
}

/// The token from the Keychain, else from `gh auth token`.
///
/// # Errors
/// [`GithubError::NoToken`] when neither has one.
pub fn token() -> Result<(String, TokenSource), GithubError> {
    if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        && let Ok(t) = entry.get_password()
        && !t.trim().is_empty()
    {
        return Ok((t, TokenSource::Keychain));
    }
    gh_token()
        .map(|t| (t, TokenSource::GhCli))
        .ok_or(GithubError::NoToken)
}

// Runs on a background thread, never on an executor, so the blocking call is fine here.
#[allow(clippy::disallowed_methods)]
fn gh_token() -> Option<String> {
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    let t = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !t.is_empty()).then_some(t)
}

/// Stores a pasted token in the macOS Keychain.
///
/// # Errors
/// [`GithubError::Keychain`] when the Keychain refuses.
pub fn save_token(token: &str) -> Result<(), GithubError> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .and_then(|e| e.set_password(token.trim()))
        .map_err(|e| GithubError::Keychain(e.to_string()))
}

/// Removes tern's token from the Keychain; `gh` is left alone.
///
/// # Errors
/// [`GithubError::Keychain`] when the Keychain refuses.
pub fn forget_token() -> Result<(), GithubError> {
    match keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .and_then(|e| e.delete_credential())
    {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(GithubError::Keychain(e.to_string())),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    version: u32,
    hash: String,
    /// Seconds since the epoch, for display.
    updated_at: u64,
    device: String,
}

/// What the gist holds, decoded.
#[derive(Debug, Clone)]
pub struct RemoteBundle {
    pub hash: String,
    pub updated_at: u64,
    pub device: String,
    pub vault: Vec<u8>,
    pub sealed: Vec<u8>,
}

#[derive(Debug)]
pub struct Gist {
    agent: ureq::Agent,
    token: String,
}

#[derive(Deserialize)]
struct GistSummary {
    id: String,
    description: Option<String>,
}

impl Gist {
    pub fn new(token: String) -> Self {
        let config = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    // The macOS trust store, not a bundled root list.
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .http_status_as_error(true)
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
            token,
        }
    }

    fn get(&self, path: &str) -> Result<Value, GithubError> {
        let mut res = self
            .agent
            .get(format!("{API}{path}"))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "tern")
            .call()?;
        res.body_mut()
            .read_json::<Value>()
            .map_err(|e| GithubError::Http(e.to_string()))
    }

    fn find(&self) -> Result<Option<String>, GithubError> {
        for page in 1..=10 {
            let list: Vec<GistSummary> =
                serde_json::from_value(self.get(&format!("/gists?per_page=100&page={page}"))?)
                    .map_err(|e| GithubError::Format(e.to_string()))?;
            if let Some(g) = list
                .iter()
                .find(|g| g.description.as_deref() == Some(DESCRIPTION))
            {
                return Ok(Some(g.id.clone()));
            }
            if list.len() < 100 {
                break;
            }
        }
        Ok(None)
    }

    /// The gist's bundle, or `None` when there is no `tern-sync` gist yet.
    ///
    /// # Errors
    /// Network, auth or format errors.
    pub fn fetch(&self) -> Result<Option<RemoteBundle>, GithubError> {
        let Some(id) = self.find()? else {
            return Ok(None);
        };
        let gist = self.get(&format!("/gists/{id}"))?;
        let file = |name: &str| -> Result<String, GithubError> {
            gist["files"][name]["content"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| GithubError::Format(format!("{name} missing")))
        };
        let meta: Meta =
            serde_json::from_str(&file(META)?).map_err(|e| GithubError::Format(e.to_string()))?;
        let decode = |name: &str| -> Result<Vec<u8>, GithubError> {
            B64.decode(file(name)?.trim())
                .map_err(|e| GithubError::Format(format!("{name}: {e}")))
        };
        Ok(Some(RemoteBundle {
            hash: meta.hash,
            updated_at: meta.updated_at,
            device: meta.device,
            vault: decode(VAULT)?,
            sealed: decode(DATA)?,
        }))
    }

    /// Writes the bundle, creating the private gist on first use.
    ///
    /// # Errors
    /// Network or auth errors.
    pub fn push(
        &self,
        hash: &str,
        device: &str,
        vault: &[u8],
        sealed: &[u8],
    ) -> Result<(), GithubError> {
        let meta = Meta {
            version: 1,
            hash: hash.to_owned(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            device: device.to_owned(),
        };
        let files = json!({
            META: { "content": serde_json::to_string_pretty(&meta).map_err(|e| GithubError::Format(e.to_string()))? },
            VAULT: { "content": B64.encode(vault) },
            DATA: { "content": B64.encode(sealed) },
        });
        let request = |method: &str, path: String, body: Value| -> Result<(), GithubError> {
            let builder = match method {
                "POST" => self.agent.post(format!("{API}{path}")),
                _ => self.agent.patch(format!("{API}{path}")),
            };
            builder
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "tern")
                .send_json(body)?;
            Ok(())
        };
        match self.find()? {
            Some(id) => request("PATCH", format!("/gists/{id}"), json!({ "files": files })),
            None => request(
                "POST",
                "/gists".into(),
                json!({ "description": DESCRIPTION, "public": false, "files": files }),
            ),
        }
    }
}
