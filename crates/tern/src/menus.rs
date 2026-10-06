// Adapted from zeron crates/ui/src/app_menus.rs (MIT).
//! The macOS menu bar and its app-wide shortcuts. Without `set_menus` macOS shows no menu
//! and ⌘Q does nothing.

use gpui::{App, KeyBinding, Window, actions};
#[cfg(target_os = "macos")]
use gpui::{Menu, MenuItem};

actions!(tern, [Quit, Hide, HideOthers, ShowAll, Minimize, Zoom]);

pub fn init(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| with_active_window(cx, |w| w.minimize_window()));
    cx.on_action(|_: &Zoom, cx| with_active_window(cx, |w| w.zoom_window()));
    // Only macOS has an application menu bar; elsewhere the chords below are all there is.
    #[cfg(target_os = "macos")]
    cx.set_menus([
        Menu {
            name: "tern".into(),
            items: vec![
                MenuItem::action("Settings…", crate::shell::OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Hide tern", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit tern", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Zoom", Zoom),
            ],
            disabled: false,
        },
    ]);
}

/// The menu's own chords; not rebindable, as in every macOS app. Windows and Linux have only
/// Quit (Ctrl+Shift+Q); there is no Hide, and the window manager minimises.
#[cfg(not(target_os = "macos"))]
pub fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new("ctrl-shift-q", Quit, None)]
}

/// The menu's own chords; not rebindable, as in every macOS app.
#[cfg(target_os = "macos")]
pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
    ]
}

fn with_active_window(cx: &mut App, f: impl FnOnce(&mut Window)) {
    if let Some(window) = cx.active_window() {
        window.update(cx, |_, window, _| f(window)).ok();
    }
}
