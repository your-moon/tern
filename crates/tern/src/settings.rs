// Adapted from zeron crates/ui/src/settings.rs (UiSettings load/save/clamped, sidebar bounds) (MIT).
//! UI preferences in `~/Library/Application Support/tern/settings.json`.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SIDEBAR_MIN: f32 = 224.0;
pub const SIDEBAR_MAX: f32 = 400.0;
pub const SIDEBAR_DEFAULT: f32 = 256.0;
const FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_DEFAULT,
            sidebar_collapsed: false,
        }
    }
}

impl Settings {
    /// Missing file → defaults. A file that does not parse is logged and replaced by defaults
    /// rather than stopping the app; fields it lacks take their defaults.
    pub fn load(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(dir.join(FILE_NAME)) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(settings) => settings.clamped(),
            Err(e) => {
                tracing::warn!(error = %e, "settings_corrupt_using_defaults");
                Self::default()
            }
        }
    }

    /// Temp file + rename, so a crash mid-write never leaves a half-written file.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE_NAME);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)
    }

    /// Widths into their legal range; NaN or infinity back to the default.
    pub fn clamped(mut self) -> Self {
        self.sidebar_width = clamp_or(
            self.sidebar_width,
            SIDEBAR_MIN,
            SIDEBAR_MAX,
            SIDEBAR_DEFAULT,
        );
        self
    }
}

/// `$TERN_CONFIG_DIR` when set (test runs keep their settings apart from the real ones),
/// else `~/Library/Application Support/tern`; `None` when there is no home directory.
pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TERN_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::home_dir().map(|home| home.join("Library/Application Support/tern"))
}

fn clamp_or(value: f32, min: f32, max: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        default
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tern-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        assert_eq!(Settings::load(&temp_dir("missing")), Settings::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let saved = Settings {
            sidebar_width: 312.0,
            sidebar_collapsed: true,
        };
        saved.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir), saved);
        assert!(!dir.join("settings.json.tmp").exists());
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), "{ not json").unwrap();
        assert_eq!(Settings::load(&dir), Settings::default());
    }

    #[test]
    fn absent_fields_take_defaults_and_unknown_fields_are_ignored() {
        let dir = temp_dir("partial");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE_NAME),
            r#"{"sidebarCollapsed":true,"later":1}"#,
        )
        .unwrap();
        let loaded = Settings::load(&dir);
        assert!(loaded.sidebar_collapsed);
        assert_eq!(loaded.sidebar_width, SIDEBAR_DEFAULT);
    }

    #[test]
    fn width_is_clamped_on_load() {
        let at = |w: f32| {
            Settings {
                sidebar_width: w,
                ..Settings::default()
            }
            .clamped()
            .sidebar_width
        };
        assert_eq!(at(10.0), SIDEBAR_MIN);
        assert_eq!(at(9000.0), SIDEBAR_MAX);
        assert_eq!(at(300.0), 300.0);
        assert_eq!(at(f32::NAN), SIDEBAR_DEFAULT);
    }
}
