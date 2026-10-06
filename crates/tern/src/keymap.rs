// Adapted from zeron crates/ui/src/settings.rs (combo_from_keystroke_on, badge_combo_on) and
// crates/ui/src/settings/shortcuts.rs (record_key, RecordOutcome, refusal) (MIT).
//! Every rebindable shortcut, the user's overrides in `keymap.json`, and the pure helpers the
//! Shortcuts page records with. ⌘ chords belong to tern; everything else reaches the remote
//! shell, so a recorded shortcut must use ⌘, ⌃ or ⌥ (or be a function key).

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use gpui::{Action, App, Global, KeyBinding};
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "keymap.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutId {
    HostPicker,
    NewConnection,
    Settings,
    ToggleSidebar,
    CloseTab,
    NextTab,
    PrevTab,
    FontBigger,
    FontSmaller,
    FontReset,
}

impl ShortcutId {
    pub const ALL: [ShortcutId; 10] = [
        ShortcutId::HostPicker,
        ShortcutId::NewConnection,
        ShortcutId::Settings,
        ShortcutId::ToggleSidebar,
        ShortcutId::CloseTab,
        ShortcutId::NextTab,
        ShortcutId::PrevTab,
        ShortcutId::FontBigger,
        ShortcutId::FontSmaller,
        ShortcutId::FontReset,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShortcutId::HostPicker => "Open host picker",
            ShortcutId::NewConnection => "New connection",
            ShortcutId::Settings => "Open settings",
            ShortcutId::ToggleSidebar => "Toggle sidebar",
            ShortcutId::CloseTab => "Close tab",
            ShortcutId::NextTab => "Next tab",
            ShortcutId::PrevTab => "Previous tab",
            ShortcutId::FontBigger => "Bigger text",
            ShortcutId::FontSmaller => "Smaller text",
            ShortcutId::FontReset => "Default text size",
        }
    }

    pub fn default_combo(self) -> &'static str {
        match self {
            ShortcutId::HostPicker => "cmd-k",
            ShortcutId::NewConnection => "cmd-n",
            ShortcutId::Settings => "cmd-,",
            ShortcutId::ToggleSidebar => "cmd-b",
            ShortcutId::CloseTab => "cmd-w",
            ShortcutId::NextTab => "cmd-shift-]",
            ShortcutId::PrevTab => "cmd-shift-[",
            ShortcutId::FontBigger => "cmd-=",
            ShortcutId::FontSmaller => "cmd--",
            ShortcutId::FontReset => "cmd-0",
        }
    }

    fn action(self) -> Box<dyn Action> {
        use crate::shell::{
            DecreaseFontSize, IncreaseFontSize, NewConnection, OpenSettings, ResetFontSize,
            ToggleSidebar,
        };
        use crate::tabs::{CloseTab, NextTab, PrevTab};
        match self {
            ShortcutId::HostPicker => Box::new(crate::picker::ToggleHostPicker),
            ShortcutId::NewConnection => Box::new(NewConnection),
            ShortcutId::Settings => Box::new(OpenSettings),
            ShortcutId::ToggleSidebar => Box::new(ToggleSidebar),
            ShortcutId::CloseTab => Box::new(CloseTab),
            ShortcutId::NextTab => Box::new(NextTab),
            ShortcutId::PrevTab => Box::new(PrevTab),
            ShortcutId::FontBigger => Box::new(IncreaseFontSize),
            ShortcutId::FontSmaller => Box::new(DecreaseFontSize),
            ShortcutId::FontReset => Box::new(ResetFontSize),
        }
    }
}

/// Chords tern keeps for itself outside the keymap: the menu's (quit, hide, minimise),
/// clipboard, and tab slots.
const RESERVED: [&str; 7] = [
    "cmd-q",
    "cmd-h",
    "alt-cmd-h",
    "cmd-m",
    "cmd-c",
    "cmd-v",
    "cmd-a",
];

/// The user's overrides; an empty string unbinds. Anything absent uses its default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap(pub BTreeMap<ShortcutId, String>);

impl Global for Keymap {}

impl Keymap {
    pub fn combo(&self, id: ShortcutId) -> &str {
        self.0
            .get(&id)
            .map_or_else(|| id.default_combo(), String::as_str)
    }

    pub fn is_default(&self, id: ShortcutId) -> bool {
        self.combo(id) == id.default_combo()
    }

    /// Sets `id` to `combo`, dropping the override when it equals the default.
    pub fn set(&mut self, id: ShortcutId, combo: &str) {
        if combo == id.default_combo() {
            self.0.remove(&id);
        } else {
            self.0.insert(id, combo.to_owned());
        }
    }

    /// Missing file → defaults; a broken one is logged and ignored.
    pub fn load(dir: &Path) -> Self {
        match std::fs::read_to_string(dir.join(FILE_NAME)) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "keymap_corrupt_using_defaults");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Temp file + rename.
    ///
    /// # Errors
    /// When the directory or file cannot be written.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE_NAME);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )?;
        std::fs::rename(&tmp, &path)
    }

    /// Why `combo` cannot go to `id`, or `None` when it can.
    pub fn refusal(&self, id: ShortcutId, combo: &str) -> Option<String> {
        if RESERVED.contains(&combo) {
            return Some(format!(
                "{} is kept by macOS or the clipboard",
                badge(combo)
            ));
        }
        ShortcutId::ALL
            .iter()
            .find(|other| **other != id && self.combo(**other) == combo)
            .map(|other| format!("{} is already “{}”", badge(combo), other.label()))
    }

    /// Key bindings for every bound shortcut.
    pub fn bindings(&self) -> Vec<KeyBinding> {
        ShortcutId::ALL
            .iter()
            .filter(|id| !self.combo(**id).is_empty())
            .map(|id| {
                KeyBinding::load(
                    self.combo(*id),
                    id.action(),
                    None,
                    false,
                    None,
                    &gpui::DummyKeyboardMapper,
                )
            })
            .filter_map(Result::ok)
            .collect()
    }
}

/// Re-installs every binding: the app's fixed ones, then the keymap's.
pub fn apply(cx: &mut App) {
    let keymap = cx.global::<Keymap>().clone();
    cx.clear_key_bindings();
    cx.bind_keys(crate::menus::bindings());
    cx.bind_keys(crate::tabs::bindings());
    cx.bind_keys(crate::text_input::bindings());
    cx.bind_keys(keymap.bindings());
}

/// Outcome of one keystroke while recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// Escape: keep the old shortcut.
    Cancelled,
    /// Backspace or Delete with no modifier: unbind.
    Cleared,
    /// A bare modifier, or a key that would reach the shell: keep listening.
    Ignored,
    Set(String),
}

pub fn record(key: &str, ctrl: bool, alt: bool, shift: bool, cmd: bool) -> Record {
    let key = key.trim().to_lowercase();
    let any_mod = ctrl || alt || cmd;
    match key.as_str() {
        "escape" if !any_mod && !shift => return Record::Cancelled,
        "backspace" | "delete" if !any_mod && !shift => return Record::Cleared,
        "" | "control" | "ctrl" | "alt" | "shift" | "cmd" | "platform" | "fn" => {
            return Record::Ignored;
        }
        _ => {}
    }
    let function_key = key.len() > 1 && key.starts_with('f') && key[1..].parse::<u8>().is_ok();
    if !any_mod && !function_key {
        return Record::Ignored;
    }
    let mut parts = Vec::new();
    if ctrl {
        parts.push("ctrl");
    }
    if alt {
        parts.push("alt");
    }
    if shift {
        parts.push("shift");
    }
    if cmd {
        parts.push("cmd");
    }
    parts.push(&key);
    Record::Set(parts.join("-"))
}

/// `cmd-shift-]` → `⇧⌘]`, in the macOS menu order ⌃⌥⇧⌘.
pub fn badge(combo: &str) -> String {
    if combo.is_empty() {
        return "—".into();
    }
    // A trailing "-" is the minus key, not a separator.
    let (mods, key) = match combo.strip_suffix("--") {
        Some(rest) => (rest, "-"),
        None => combo.rsplit_once('-').unwrap_or(("", combo)),
    };
    let parts: Vec<&str> = mods.split('-').collect();
    let mut out = String::new();
    for (name, glyph) in [("ctrl", '⌃'), ("alt", '⌥'), ("shift", '⇧'), ("cmd", '⌘')] {
        if parts.contains(&name) {
            out.push(glyph);
        }
    }
    let mut chars = key.chars();
    if let Some(first) = chars.next() {
        out.extend(first.to_uppercase());
        out.push_str(chars.as_str());
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn recording_needs_a_modifier_and_orders_it() {
        assert_eq!(
            record("k", false, false, false, true),
            Record::Set("cmd-k".into())
        );
        assert_eq!(
            record("P", true, true, true, true),
            Record::Set("ctrl-alt-shift-cmd-p".into())
        );
        assert_eq!(record("k", false, false, false, false), Record::Ignored);
        assert_eq!(record("shift", false, false, true, false), Record::Ignored);
        assert_eq!(
            record("f5", false, false, false, false),
            Record::Set("f5".into())
        );
        assert_eq!(
            record("escape", false, false, false, false),
            Record::Cancelled
        );
        assert_eq!(
            record("backspace", false, false, false, false),
            Record::Cleared
        );
    }

    #[test]
    fn badges_read_like_macos_menus() {
        assert_eq!(badge("cmd-shift-]"), "⇧⌘]");
        assert_eq!(badge("cmd--"), "⌘-");
        assert_eq!(badge("ctrl-alt-cmd-k"), "⌃⌥⌘K");
        assert_eq!(badge(""), "—");
    }

    #[test]
    fn overrides_fall_back_to_defaults_and_conflicts_name_the_owner() {
        let mut k = Keymap::default();
        assert_eq!(k.combo(ShortcutId::HostPicker), "cmd-k");
        k.set(ShortcutId::HostPicker, "cmd-p");
        assert_eq!(k.combo(ShortcutId::HostPicker), "cmd-p");
        assert!(!k.is_default(ShortcutId::HostPicker));
        let refusal = k.refusal(ShortcutId::Settings, "cmd-p").unwrap();
        assert!(refusal.contains("Open host picker"), "{refusal}");
        assert!(k.refusal(ShortcutId::Settings, "cmd-k").is_none());
        // Re-recording a shortcut's own chord is not a conflict with itself.
        assert!(k.refusal(ShortcutId::HostPicker, "cmd-p").is_none());
        assert!(k.refusal(ShortcutId::Settings, "cmd-q").is_some());
        k.set(ShortcutId::HostPicker, "cmd-k");
        assert!(k.0.is_empty());
    }

    #[test]
    fn every_default_binds() {
        let k = Keymap::default();
        assert_eq!(k.bindings().len(), ShortcutId::ALL.len());
        let mut k2 = Keymap::default();
        k2.set(ShortcutId::CloseTab, "");
        assert_eq!(k2.bindings().len(), ShortcutId::ALL.len() - 1);
    }

    #[test]
    fn keymap_round_trips_through_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut k = Keymap::default();
        k.set(ShortcutId::ToggleSidebar, "cmd-\\");
        k.save(dir.path()).unwrap();
        assert_eq!(Keymap::load(dir.path()), k);
    }
}
