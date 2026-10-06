//! Update check: once a day at launch, ask GitHub Releases for the latest tern and, when it is
//! newer than this build, offer a toast whose action opens the release page. tern never
//! downloads or replaces itself; the user installs the release (or `brew upgrade`) as usual.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{AppContext as _, Context};
use serde::Deserialize;

use super::{Shell, Toast, ToastKind};
use crate::settings;

const LATEST_URL: &str = "https://api.github.com/repos/your-moon/tern/releases/latest";
/// Only a page on GitHub is ever opened, whatever the response says.
const PAGE_PREFIX: &str = "https://github.com/";
const STAMP_FILE: &str = "update-check";
const INTERVAL_SECS: u64 = 24 * 60 * 60;

/// `major.minor.patch`; compared field by field, so 0.10.0 is newer than 0.9.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

impl Version {
    /// `v0.2.0` or `0.2.0`. A pre-release (`0.2.0-rc.1`) is `None`: tern only announces
    /// finished releases: `-rc.1` makes the patch field or the field count wrong, so it
    /// fails to parse. Build metadata (`+abc`) is ignored.
    fn parse(tag: &str) -> Option<Self> {
        let tag = tag.trim().trim_start_matches('v');
        let tag = tag.split('+').next()?;
        let mut parts = tag.split('.');
        let v = Version(
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        parts.next().is_none().then_some(v)
    }
}

/// The version to announce: the release's, when it parses, is not a pre-release and is newer.
fn newer(current: &str, latest_tag: &str) -> Option<String> {
    let current = Version::parse(current)?;
    let latest = Version::parse(latest_tag)?;
    (latest > current).then(|| latest_tag.trim().trim_start_matches('v').to_string())
}

/// At most one check a day. A clock that went backwards counts as due, so a wrong date never
/// silences the check for years.
fn due(last: Option<u64>, now: u64) -> bool {
    match last {
        Some(last) if last <= now => now - last >= INTERVAL_SECS,
        _ => true,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn last_check(dir: &Path) -> Option<u64> {
    std::fs::read_to_string(dir.join(STAMP_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
}

/// What the toast needs: the version text and the page to open.
struct Available {
    version: String,
    url: String,
}

/// Blocking; runs on the background executor.
fn fetch(current: &str) -> Result<Option<Available>, String> {
    let config = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(true)
        .build();
    let release: Release = ureq::Agent::new_with_config(config)
        .get(LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "tern")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    if release.prerelease || release.draft || !release.html_url.starts_with(PAGE_PREFIX) {
        return Ok(None);
    }
    Ok(newer(current, &release.tag_name).map(|version| Available {
        version,
        url: release.html_url,
    }))
}

impl Shell {
    /// Runs the daily check when the setting allows it. The attempt is stamped even when it
    /// fails, so being offline does not retry on every launch.
    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if !self.settings.check_for_updates {
            return;
        }
        let Some(dir) = settings::dir() else { return };
        let now = now_secs();
        if !due(last_check(&dir), now) {
            return;
        }
        let work = cx.background_spawn(async move {
            let _ = std::fs::create_dir_all(&dir);
            if let Err(e) = std::fs::write(dir.join(STAMP_FILE), now.to_string()) {
                tracing::warn!(error = %e, "update_stamp_failed");
            }
            fetch(env!("CARGO_PKG_VERSION"))
        });
        cx.spawn(async move |this, cx| match work.await {
            Ok(Some(found)) => {
                let _ = this.update(cx, |s, cx| {
                    let url = found.url;
                    s.toast(
                        Toast::new(
                            ToastKind::Default,
                            format!("tern {} is available", found.version),
                        )
                        .action("Download", move |_, _, cx| cx.open_url(&url)),
                        cx,
                    );
                });
            }
            Ok(None) => tracing::debug!("update_check_current"),
            Err(e) => tracing::info!(error = %e, "update_check_failed"),
        })
        .detach();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn newer_compares_numbers_not_text() {
        // (running, latest tag, announced)
        let cases = [
            ("0.9.0", "v0.10.0", Some("0.10.0")),
            ("0.10.0", "v0.9.0", None),
            ("0.1.0", "v0.1.1", Some("0.1.1")),
            ("0.1.0", "v0.1.0", None),
            ("0.2.9", "1.0.0", Some("1.0.0")),
            ("1.9.9", "v1.10.0", Some("1.10.0")),
            ("1.2.3", "v1.2.3+build5", None),
        ];
        for (running, tag, want) in cases {
            assert_eq!(newer(running, tag).as_deref(), want, "{running} vs {tag}");
        }
    }

    #[test]
    fn prereleases_and_junk_are_ignored() {
        for tag in [
            "v0.3.0-rc.1",
            "0.3.0-beta",
            "v0.3",
            "v0.3.0.1",
            "nightly",
            "",
        ] {
            assert_eq!(newer("0.1.0", tag), None, "{tag}");
        }
    }

    #[test]
    fn the_check_runs_at_most_daily() {
        let now = 10 * INTERVAL_SECS;
        assert!(due(None, now));
        assert!(!due(Some(now - 60), now));
        assert!(!due(Some(now - INTERVAL_SECS + 1), now));
        assert!(due(Some(now - INTERVAL_SECS), now));
        // The clock moved back past the stamp.
        assert!(due(Some(now + 5), now));
    }

    #[test]
    fn the_stamp_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(last_check(dir.path()), None);
        std::fs::write(dir.path().join(STAMP_FILE), "1760000000\n").ok();
        assert_eq!(last_check(dir.path()), Some(1_760_000_000));
    }
}
