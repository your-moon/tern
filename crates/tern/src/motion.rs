//! Whether to hold animations still: tern's own "Reduce motion" setting, or macOS's
//! (System Settings → Accessibility → Display → Reduce motion), as zeron reads it in
//! crates/ui/src/motion.rs. objc2-app-kit exposes both calls as safe functions, so the
//! workspace's `unsafe_code = "forbid"` holds.

use gpui::App;

/// macOS's Reduce motion preference.
#[cfg(target_os = "macos")]
pub fn system_reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
pub fn system_reduce_motion() -> bool {
    false
}

/// Applies the effective value: either switch on holds motion still.
pub fn apply(setting: bool, cx: &mut App) {
    let reduce = setting || system_reduce_motion();
    if cx.reduce_motion() != reduce {
        cx.set_reduce_motion(reduce);
    }
}

#[cfg(all(test, target_os = "macos"))]
#[allow(clippy::disallowed_methods, clippy::expect_used)]
mod tests {
    /// The safe binding reads the same preference `defaults` shows (absent means off).
    #[test]
    fn system_value_matches_the_preference_store() {
        let out = std::process::Command::new("defaults")
            .args(["read", "com.apple.universalaccess", "reduceMotion"])
            .output()
            .expect("defaults");
        let stored = String::from_utf8_lossy(&out.stdout).trim() == "1";
        assert_eq!(super::system_reduce_motion(), stored);
    }
}
