//! Behaviour switches for a terminal view, gathered in one struct so the app
//! can pass its settings through in one call
//! ([`TerminalView::set_options`](crate::TerminalView::set_options)).
//! `Default` is the behaviour before any option existed.

use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle};

use crate::terminal::SCROLLBACK_LINES;

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
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            cursor_style: CursorStyleSetting::Block,
            cursor_blink: false,
            scrollback_lines: SCROLLBACK_LINES,
        }
    }
}
