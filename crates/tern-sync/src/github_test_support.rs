//! Test doubles for the gist side: seams that fail loudly when a test forgets to configure one,
//! and a scripted `gh`.

use super::{GhCli, GhOutput, GithubError, GithubHttp, Keychain, Method};
use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct UnsetKeychain;

impl Keychain for UnsetKeychain {
    fn get(&self) -> Result<String, keyring::Error> {
        Err(keyring::Error::PlatformFailure(Box::new(
            std::io::Error::other("keychain not configured in test"),
        )))
    }

    fn set(&self, _token: &str) -> Result<(), keyring::Error> {
        Err(keyring::Error::PlatformFailure(Box::new(
            std::io::Error::other("keychain not configured in test"),
        )))
    }

    fn delete(&self) -> Result<(), keyring::Error> {
        Err(keyring::Error::PlatformFailure(Box::new(
            std::io::Error::other("keychain not configured in test"),
        )))
    }

    fn clone_box(&self) -> Box<dyn Keychain> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone)]
pub(super) struct UnsetGh;

impl GhCli for UnsetGh {
    fn run(&self, _args: &[&str]) -> Result<GhOutput, std::io::Error> {
        Err(std::io::Error::other("gh cli not configured in test"))
    }

    fn clone_box(&self) -> Box<dyn GhCli> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone)]
pub(super) struct UnsetHttp;

impl GithubHttp for UnsetHttp {
    fn get(&self, _token: &str, _path: &str) -> Result<Value, GithubError> {
        Err(GithubError::Http("http not configured in test".to_owned()))
    }

    fn send(
        &self,
        _token: &str,
        _method: Method,
        _path: &str,
        _body: Value,
    ) -> Result<(), GithubError> {
        Err(GithubError::Http("http not configured in test".to_owned()))
    }

    fn clone_box(&self) -> Box<dyn GithubHttp> {
        Box::new(self.clone())
    }
}

type GhResponseQueue = std::collections::VecDeque<Result<GhOutput, String>>;

#[derive(Debug, Default, Clone)]
pub(crate) struct FakeGh {
    pub(crate) responses:
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, GhResponseQueue>>>,
    pub(crate) calls: std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
}

#[allow(clippy::unwrap_used)]
impl FakeGh {
    pub(crate) fn respond(&self, key: impl Into<String>, res: Result<GhOutput, String>) {
        self.responses
            .lock()
            .unwrap()
            .entry(key.into())
            .or_default()
            .push_back(res);
    }
}

#[allow(clippy::unwrap_used)]
impl GhCli for FakeGh {
    fn run(&self, args: &[&str]) -> Result<GhOutput, std::io::Error> {
        let key = args.join(" ");
        self.calls
            .lock()
            .unwrap()
            .push(args.iter().map(|s| (*s).to_owned()).collect());
        let mut map = self.responses.lock().unwrap();
        if let Some(queue) = map.get_mut(&key)
            && let Some(res) = queue.pop_front()
        {
            return match res {
                Ok(out) => Ok(out),
                Err(e) => Err(std::io::Error::other(e)),
            };
        }
        Ok(GhOutput {
            success: false,
            stdout: Vec::new(),
            stderr: b"command failed".to_vec(),
        })
    }

    fn clone_box(&self) -> Box<dyn GhCli> {
        Box::new(self.clone())
    }
}
