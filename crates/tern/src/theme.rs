// Adapted from zeron crates/theme/src/builtins.rs `zeron_dark` and crates/ui/src/theme.rs (MIT).
//! The Zeron Dark palette and layout tokens, values copied from zeron's source.

use gpui::{Hsla, hsla, rgb};

/// Window frost over the blurred desktop (zeron `Theme::GLASS_ALPHA`, macOS).
pub const GLASS_ALPHA: f32 = 0.80;
pub const TITLEBAR_HEIGHT: f32 = 38.0;
pub const TITLEBAR_TOP_PAD: f32 = 4.0;
pub const PANEL_RADIUS: f32 = 10.0;
pub const SPACE_SM: f32 = 8.0;
pub const UI_FONT: &str = "Geist";

/// Where titlebar content starts: clear of the traffic lights, or the edge in fullscreen
/// (zeron `titlebar_cluster_start`).
pub fn titlebar_content_start(fullscreen: bool) -> f32 {
    if fullscreen { 12.0 } else { 88.0 }
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub shell: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub border: Hsla,
    pub terminal_background: Hsla,
}

impl Theme {
    pub fn zeron_dark() -> Self {
        Self {
            shell: hex(0x0d0d0d),
            text: hex(0xe8e8ea),
            muted: hex(0xa9a9ae),
            border: hsla(0.0, 0.0, 1.0, 0.08),
            terminal_background: hex(0x090909),
        }
    }

    /// The shell surface as frost: the blurred desktop shows through at 1 - `GLASS_ALPHA`.
    pub fn glass(&self) -> Hsla {
        self.shell.opacity(GLASS_ALPHA)
    }
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}
