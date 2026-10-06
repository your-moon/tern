//! Behaviour switches for a terminal view, gathered in one struct so the app
//! can pass its settings through in one call
//! ([`TerminalView::set_options`](crate::TerminalView::set_options)).
//! `Default` is the behaviour before any option existed.

use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle};

use crate::terminal::{SCROLLBACK_LINES, TerminalEvent};

/// The cursor drawn until the remote picks its own with DECSCUSR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyleSetting {
    #[default]
    Block,
    Bar,
    Underline,
}

impl CursorStyleSetting {
    pub(crate) fn shape(self) -> CursorShape {
        match self {
            Self::Block => CursorShape::Block,
            Self::Bar => CursorShape::Beam,
            Self::Underline => CursorShape::Underline,
        }
    }

    /// The emulator-side default style for this setting.
    pub(crate) fn style(self, blink: bool) -> CursorStyle {
        CursorStyle {
            shape: self.shape(),
            blinking: blink,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalOptions {
    /// Cursor shape until the remote sends DECSCUSR; `CSI 0 SP q` returns here.
    pub cursor_style: CursorStyleSetting,
    /// Whether the default cursor blinks. The remote's DECSCUSR (odd = blink,
    /// even = steady) overrides it until reset.
    pub cursor_blink: bool,
    /// Lines of scrollback kept client-side.
    pub scrollback_lines: usize,
    /// Finishing a mouse selection copies it to the clipboard.
    pub copy_on_select: bool,
    /// Middle-click pastes the clipboard (when the remote is not taking mouse
    /// reports).
    pub middle_click_paste: bool,
    /// Flash the view on BEL. The `Bell` event is emitted either way, so the
    /// app can bounce the Dock regardless.
    pub visual_bell: bool,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            cursor_style: CursorStyleSetting::Block,
            cursor_blink: false,
            scrollback_lines: SCROLLBACK_LINES,
            copy_on_select: false,
            middle_click_paste: false,
            visual_bell: false,
        }
    }
}

impl TerminalOptions {
    /// Whether `event` should flash the view.
    pub(crate) fn flashes_on(&self, event: &TerminalEvent) -> bool {
        self.visual_bell && matches!(event, TerminalEvent::Bell)
    }

    /// Whether releasing the mouse should copy: copy-on-select is on, it was
    /// the left button, the press turned into a real selection gesture (a plain
    /// click that only focused the view does not count), and a selection exists.
    pub(crate) fn copies_on_release(&self, left: bool, dragged: bool, selected: bool) -> bool {
        self.copy_on_select && left && dragged && selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_on_select_needs_the_option_and_a_real_selection() {
        let on = TerminalOptions {
            copy_on_select: true,
            ..TerminalOptions::default()
        };
        assert!(on.copies_on_release(true, true, true));
        assert!(!TerminalOptions::default().copies_on_release(true, true, true));
        assert!(
            !on.copies_on_release(false, true, true),
            "not the left button"
        );
        assert!(!on.copies_on_release(true, false, true), "focusing click");
        assert!(!on.copies_on_release(true, true, false), "nothing selected");
    }

    #[test]
    fn only_a_bell_flashes_and_only_when_enabled() {
        let on = TerminalOptions {
            visual_bell: true,
            ..TerminalOptions::default()
        };
        assert!(on.flashes_on(&TerminalEvent::Bell));
        assert!(!on.flashes_on(&TerminalEvent::TitleChanged("x".into())));
        assert!(!TerminalOptions::default().flashes_on(&TerminalEvent::Bell));
    }

    #[test]
    fn defaults_keep_todays_behaviour() {
        let d = TerminalOptions::default();
        assert!(!d.copy_on_select && !d.middle_click_paste && !d.cursor_blink);
        assert_eq!(d.cursor_style, CursorStyleSetting::Block);
        assert_eq!(d.scrollback_lines, 10_000);
    }
}
