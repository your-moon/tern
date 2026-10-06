//! What the "show this file" action is called: the file manager differs by system.

/// The label for `cx.reveal_path`'s button.
pub const LABEL: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else if cfg!(target_os = "windows") {
    "Show in Explorer"
} else {
    "Show in Files"
};
