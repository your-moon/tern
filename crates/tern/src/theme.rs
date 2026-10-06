// Adapted from zeron crates/theme/src/builtins.rs `zeron_dark` / `zeron_light` and
// crates/ui/src/theme.rs (MIT).
//! The Zeron Dark and Light palettes and layout tokens, values copied from zeron's source.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Hsla, Pixels, hsla, px, rgb};
use tern_term::TerminalTheme;

use crate::themes::Scheme;

/// Window frost over the blurred desktop (zeron `Theme::GLASS_ALPHA`, macOS).
pub const GLASS_ALPHA: f32 = 0.80;
pub const TITLEBAR_HEIGHT: f32 = 38.0;
pub const TITLEBAR_TOP_PAD: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
/// zeron `Theme::SPACE_LG` (crates/ui/src/theme.rs:842): the gap between nav groups.
pub const SPACE_LG: f32 = 16.0;
pub const UI_FONT: &str = "Geist";
pub const MONO_FONT: &str = "Geist Mono";
pub const CONTROL_RADIUS: f32 = 6.0;

/// Whether the window is on a standard-density display (scale factor below 1.5) with sharper
/// text on. Set once per frame by `Shell::render`, so moving the window to another monitor
/// switches modes on the next frame; Retina never sees any of the helpers below change a value.
static LOW_DPI: AtomicBool = AtomicBool::new(false);

/// The scale factor under which a display counts as standard density.
pub const LOW_DPI_BELOW: f32 = 1.5;

pub fn set_low_dpi(on: bool) {
    LOW_DPI.store(on, Ordering::Relaxed);
}

pub fn low_dpi() -> bool {
    LOW_DPI.load(Ordering::Relaxed)
}

/// `v` rounded to a whole pixel at 1x, where a fraction puts glyph and icon edges on half
/// pixels; untouched on Retina.
pub fn snap(v: f32) -> f32 {
    if low_dpi() { v.round() } else { v }
}

/// `snap` as `Pixels`.
pub fn crisp(v: f32) -> Pixels {
    px(snap(v))
}

/// An icon size that centres in `outer` on whole pixels: at 1x an odd gap between box and icon
/// would leave the icon on a half pixel, so the icon grows by one.
pub fn fit(outer: f32, size: f32) -> f32 {
    if low_dpi() && ((outer - size).abs() % 2.0) > 0.5 {
        size + 1.0
    } else {
        size
    }
}

/// `retina` normally, `standard` on a 1x display: used for opacities that need a step more
/// contrast where grayscale-only text is thin.
pub fn by_dpi(retina: f32, standard: f32) -> f32 {
    if low_dpi() { standard } else { retina }
}

const ANSI_DARK: [u32; 16] = [
    0x242424, 0xf87171, 0x4ade80, 0xfacc15, 0x60a5fa, 0xc084fc, 0x22d3ee, 0xd4d4d8, 0x52525b,
    0xfca5a5, 0x86efac, 0xfde047, 0x93c5fd, 0xd8b4fe, 0x67e8f9, 0xfafafa,
];

/// zeron `ANSI_LIGHT` (crates/theme/src/builtins.rs:234-237).
const ANSI_LIGHT: [u32; 16] = [
    0x1f1f1f, 0xdc2626, 0x16a34a, 0xb45309, 0x2563eb, 0x9333ea, 0x0e7490, 0x3f3f46, 0x71717a,
    0xb91c1c, 0x15803d, 0x92400e, 0x1d4ed8, 0x7e22ce, 0x155e75, 0x18181b,
];

/// zeron `INK_HAIRLINE_SCALE` (crates/ui/src/theme.rs:374): light hairlines carry 1.35x the
/// alpha so a 1 px edge survives a bright surround.
const INK_HAIRLINE_SCALE: f32 = 1.35;
/// zeron `INK_FILL_SCALE` (crates/ui/src/theme.rs:368).
const INK_FILL_SCALE: f32 = 1.0;
/// zeron `SCRIM_ALPHA_DARK` (crates/ui/src/theme.rs:1734): the dark-mode modal backdrop.
pub const SCRIM_ALPHA_DARK: f32 = 0.60;

/// Hairline ink: white on dark, black on light (zeron `hairline_for`, theme.rs:1715-1721).
fn hairline_for(light: bool, alpha: f32) -> Hsla {
    if light {
        hsla(0.0, 0.0, 0.0, (alpha * INK_HAIRLINE_SCALE).min(0.5))
    } else {
        hsla(0.0, 0.0, 1.0, alpha)
    }
}

/// Interactive-state wash (zeron `wash_for`, theme.rs:1728-1733).
fn wash_for(light: bool, alpha: f32) -> Hsla {
    if light {
        hsla(0.0, 0.0, 0.10, alpha * INK_FILL_SCALE)
    } else {
        hsla(0.0, 0.0, 0.92, alpha)
    }
}

/// Where titlebar content starts: clear of the traffic lights, or the edge in fullscreen
/// (zeron `titlebar_cluster_start`).
pub fn titlebar_content_start(fullscreen: bool) -> f32 {
    if fullscreen { 12.0 } else { 88.0 }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Whether this is the light palette.
    pub light: bool,
    pub shell: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub accent: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    /// Amber: degraded but working (a slow link).
    pub warning: Hsla,
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
    /// The wallpaper tint mixed into the surfaces, when theme colours from the wallpaper are
    /// on (see `theme_tint`); terminal schemes are mixed with it too.
    pub tint: Option<[u8; 3]>,
}

impl Theme {
    pub fn zeron(light: bool) -> Self {
        if light {
            Self::zeron_light()
        } else {
            Self::zeron_dark()
        }
    }

    /// zeron `zeron_light` (crates/theme/src/builtins.rs:273-304); washes, hairlines and the
    /// popup from crates/ui/src/theme.rs:1715-1733 (`popup` is the variant's `card`, #ffffff).
    pub fn zeron_light() -> Self {
        Self {
            light: true,
            shell: hex(0xf3f3f5),
            text: hex(0x303035),
            muted: hex(0x62626a),
            faint: hex(0x797981),
            accent: hex(0x5b43e8),
            success: hex(0x15803d),
            danger: hex(0xdc2626),
            warning: hex(0xb45309),
            border: hairline_for(true, 0.08),
            row_active: wash_for(true, 0.10),
            row_hover: wash_for(true, 0.05),
            hairline: hairline_for(true, 0.06),
            popup: hex(0xffffff),
            terminal_background: hex(0xfafafa),
            tint: None,
        }
    }

    pub fn zeron_dark() -> Self {
        Self {
            light: false,
            shell: hex(0x0d0d0d),
            text: hex(0xe8e8ea),
            muted: hex(0xa9a9ae),
            faint: hex(0x85858a),
            accent: hex(0x8b7cf6),
            success: hex(0x34d399),
            danger: hex(0xf87171),
            warning: hex(0xfbbf24),
            border: hairline_for(false, 0.08),
            row_active: wash_for(false, 0.10),
            row_hover: wash_for(false, 0.05),
            hairline: hairline_for(false, 0.06),
            popup: hsla(0.0, 0.0, 0.09, 1.0),
            terminal_background: hex(0x090909),
            tint: None,
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
            None if self.light => ANSI_LIGHT,
            None => ANSI_DARK,
        };
        for (slot, color) in t.ansi.iter_mut().zip(ansi) {
            *slot = hex(color);
        }
        if let Some(tint) = self.tint {
            crate::theme_tint::tint_terminal(&mut t, tint, self.light);
        }
        t
    }

    /// Ink at `alpha`: zeron's `wash`, a pale grey over dark surfaces, a soft black over light.
    pub fn ink(&self, alpha: f32) -> Hsla {
        wash_for(self.light, alpha)
    }

    /// Modal backdrop at `alpha_dark` (quoted in dark-mode terms): black in both, about half
    /// as strong on light (zeron `scrim_for`, theme.rs:1750-1755).
    pub fn scrim(&self, alpha_dark: f32) -> Hsla {
        if self.light {
            hsla(0.0, 0.0, 0.0, 0.32 * (alpha_dark / SCRIM_ALPHA_DARK))
        } else {
            hsla(0.0, 0.0, 0.0, alpha_dark)
        }
    }

    /// The shell surface as frost: the blurred desktop shows through at 1 - `GLASS_ALPHA`.
    pub fn glass(&self) -> Hsla {
        self.shell.opacity(GLASS_ALPHA)
    }

    /// The window fill: frost, except on a standard-density display, where the translucent
    /// fill lowers text contrast and the plain shell colour is used.
    pub fn surface(&self) -> Hsla {
        if low_dpi() { self.shell } else { self.glass() }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::wallpaper_fx::contrast_ratio;

    fn rgb_of(c: Hsla) -> u32 {
        let c = gpui::Rgba::from(c);
        let ch = |v: f32| (v * 255.0).round() as u32;
        ch(c.r) << 16 | ch(c.g) << 8 | ch(c.b)
    }

    #[test]
    fn light_palette_is_zerons() {
        let t = Theme::zeron_light();
        assert!(t.light);
        assert_eq!(rgb_of(t.shell), 0xf3f3f5);
        assert_eq!(rgb_of(t.text), 0x303035);
        assert_eq!(rgb_of(t.muted), 0x62626a);
        assert_eq!(rgb_of(t.faint), 0x797981);
        assert_eq!(rgb_of(t.accent), 0x5b43e8);
        assert_eq!(rgb_of(t.terminal_background), 0xfafafa);
        assert_eq!(rgb_of(t.popup), 0xffffff);
        assert!(!Theme::zeron_dark().light);
        assert!(Theme::zeron(true).light && !Theme::zeron(false).light);
    }

    #[test]
    fn washes_and_hairlines_flip_tone_with_appearance() {
        let (dark, light) = (Theme::zeron_dark(), Theme::zeron_light());
        // Dark: pale ink; light: soft black at the same alpha.
        assert!(dark.ink(0.1).l > 0.9 && light.ink(0.1).l < 0.2);
        assert!((light.ink(0.1).a - 0.1).abs() < 1e-6);
        // Light hairlines carry 1.35x the alpha; dark ones are unscaled.
        assert!((light.hairline.a - 0.06 * 1.35).abs() < 1e-6 && light.hairline.l == 0.0);
        assert!((dark.hairline.a - 0.06).abs() < 1e-6 && dark.hairline.l == 1.0);
        assert!((light.border.a - 0.08 * 1.35).abs() < 1e-6);
        // The scaled alpha is capped at 0.5.
        assert_eq!(hairline_for(true, 0.9).a, 0.5);
    }

    #[test]
    fn scrim_is_black_and_gentler_on_light() {
        let (dark, light) = (Theme::zeron_dark(), Theme::zeron_light());
        assert_eq!(dark.scrim(0.6).a, 0.6);
        assert!((light.scrim(0.6).a - 0.32).abs() < 1e-6);
        assert!((light.scrim(0.3).a - 0.16).abs() < 1e-6);
        assert_eq!(light.scrim(0.6).l, 0.0);
    }

    #[test]
    fn text_is_readable_on_its_surfaces_in_both_appearances() {
        for t in [Theme::zeron_dark(), Theme::zeron_light()] {
            let term = t.terminal(13.0, None);
            let shell = rgb_of(t.shell);
            assert!(contrast_ratio(rgb_of(t.text), shell) >= 7.0);
            assert!(contrast_ratio(rgb_of(t.muted), shell) >= 4.5);
            assert!(contrast_ratio(rgb_of(term.foreground), rgb_of(term.background)) >= 7.0);
            assert!(contrast_ratio(rgb_of(t.accent), shell) >= 3.0);
        }
    }

    #[test]
    fn default_terminal_palette_follows_the_appearance() {
        let dark = Theme::zeron_dark().terminal(13.0, None);
        let light = Theme::zeron_light().terminal(13.0, None);
        assert_eq!(rgb_of(dark.ansi[1]), 0xf87171);
        assert_eq!(rgb_of(light.ansi[1]), 0xdc2626);
        assert_eq!(rgb_of(light.ansi[15]), 0x18181b);
        assert_eq!(rgb_of(light.background), 0xfafafa);
    }

    #[test]
    fn a_chosen_scheme_stays_as_chosen_in_light_mode() {
        let scheme = crate::themes::find("Dracula").unwrap();
        let on_light = Theme::zeron_light().terminal(13.0, Some(scheme));
        let on_dark = Theme::zeron_dark().terminal(13.0, Some(scheme));
        assert_eq!(on_light.background, on_dark.background);
        assert_eq!(on_light.ansi, on_dark.ansi);
        assert_eq!(rgb_of(on_light.background), scheme.background);
    }
}
