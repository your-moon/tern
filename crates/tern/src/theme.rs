// Adapted from zeron crates/theme/src/builtins.rs `zeron_dark` and crates/ui/src/theme.rs (MIT).
//! The Zeron Dark palette and layout tokens, values copied from zeron's source.

use gpui::{Hsla, hsla, rgb};
use tern_term::TerminalTheme;

use crate::themes::Scheme;

/// Window frost over the blurred desktop (zeron `Theme::GLASS_ALPHA`, macOS).
pub const GLASS_ALPHA: f32 = 0.80;
pub const TITLEBAR_HEIGHT: f32 = 38.0;
pub const TITLEBAR_TOP_PAD: f32 = 4.0;
pub const PANEL_RADIUS: f32 = 10.0;
pub const SPACE_SM: f32 = 8.0;
pub const UI_FONT: &str = "Geist";
pub const MONO_FONT: &str = "Geist Mono";
pub const CONTROL_RADIUS: f32 = 6.0;

const ANSI_DARK: [u32; 16] = [
    0x242424, 0xf87171, 0x4ade80, 0xfacc15, 0x60a5fa, 0xc084fc, 0x22d3ee, 0xd4d4d8, 0x52525b,
    0xfca5a5, 0x86efac, 0xfde047, 0x93c5fd, 0xd8b4fe, 0x67e8f9, 0xfafafa,
];

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
    pub faint: Hsla,
    pub accent: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub border: Hsla,
    /// Row washes (zeron `wash(0.10)` active, `wash(0.05)` hover).
    pub row_active: Hsla,
    pub row_hover: Hsla,
    /// Divider inside floating cards (zeron `hairline(0.06)`, dark).
    pub hairline: Hsla,
    /// Floating card fill. zeron frosts its palette over a 16 px backdrop blur; tern has no
    /// per-element blur, so this is the shell colour lifted and opaque: any transparency lets
    /// the panel's text show through unblurred.
    pub popup: Hsla,
    pub terminal_background: Hsla,
}

impl Theme {
    pub fn zeron_dark() -> Self {
        Self {
            shell: hex(0x0d0d0d),
            text: hex(0xe8e8ea),
            muted: hex(0xa9a9ae),
            faint: hex(0x85858a),
            accent: hex(0x8b7cf6),
            success: hex(0x34d399),
            danger: hex(0xf87171),
            border: hsla(0.0, 0.0, 1.0, 0.08),
            row_active: hsla(0.0, 0.0, 0.92, 0.10),
            row_hover: hsla(0.0, 0.0, 0.92, 0.05),
            hairline: hsla(0.0, 0.0, 1.0, 0.06),
            popup: hsla(0.0, 0.0, 0.09, 1.0),
            terminal_background: hex(0x090909),
        }
    }

    /// The terminal palette: a bundled scheme when one is chosen, else zeron's dark one.
    pub fn terminal(&self, font_size: f32, scheme: Option<&Scheme>) -> TerminalTheme {
        let mut t = TerminalTheme {
            background: self.terminal_background,
            foreground: self.text,
            cursor: self.text,
            selection: self.accent.opacity(0.35),
            font_family: MONO_FONT.into(),
            font_size,
            ..TerminalTheme::default()
        };
        let ansi = match scheme {
            Some(s) => {
                t.background = hex(s.background);
                t.foreground = hex(s.foreground);
                t.cursor = hex(s.cursor);
                t.selection = hex(s.selection).opacity(0.6);
                s.ansi
            }
            None => ANSI_DARK,
        };
        for (slot, color) in t.ansi.iter_mut().zip(ansi) {
            *slot = hex(color);
        }
        t
    }

    /// Ink at `alpha`: zeron's dark `wash`, a light grey laid over the surface.
    pub fn ink(&self, alpha: f32) -> Hsla {
        hsla(0.0, 0.0, 0.92, alpha)
    }

    /// The shell surface as frost: the blurred desktop shows through at 1 - `GLASS_ALPHA`.
    pub fn glass(&self) -> Hsla {
        self.shell.opacity(GLASS_ALPHA)
    }
}

/// `fg` composited over `bg`, as an opaque colour (zeron `theme::flatten`).
pub fn flatten(fg: Hsla, bg: Hsla) -> Hsla {
    let (f, b) = (gpui::Rgba::from(fg), gpui::Rgba::from(bg));
    let a = fg.a;
    gpui::Rgba {
        r: f.r * a + b.r * (1.0 - a),
        g: f.g * a + b.g * (1.0 - a),
        b: f.b * a + b.b * (1.0 - a),
        a: 1.0,
    }
    .into()
}

pub fn hex(value: u32) -> Hsla {
    rgb(value).into()
}
