// Adapted from zeron crates/ui/src/settings/wallpaper_colors.rs `tint_variant` (the 40% mix toward
// a black/white pre-mixed dominant colour) and crates/ui/src/theme.rs `harden_model_foreground`
// (text pushed until it keeps contrast on every surface) (MIT).
//! The window's colours taken from the wallpaper: every surface leans toward the picture's
//! dominant colour, the accent follows it, and the text is pushed until it stays readable on
//! all of them. With the picture only a hero, this is what carries it across the window.

use gpui::{Hsla, hsla};
use tern_term::TerminalTheme;

use crate::theme::{Theme, hex};
use crate::wallpaper_colors::accent_for;
use crate::wallpaper_fx::contrast_ratio;

/// How far the dominant colour is pre-mixed toward black in dark mode (zeron `tint_variant`).
const DARK_PREMIX: f32 = 0.88;
/// ...and toward white in light mode.
const LIGHT_PREMIX: f32 = 0.94;
/// How far each surface moves toward that tint.
const SURFACE_MIX: f32 = 0.4;
/// WCAG AA for body text: what text, muted and faint text keep on every surface.
pub const TEXT_CONTRAST: f32 = 4.5;

fn pack(c: [u8; 3]) -> u32 {
    u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])
}

pub(crate) fn rgb_of(color: Hsla) -> [u8; 3] {
    let c = gpui::Rgba::from(color);
    [c.r, c.g, c.b].map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn from_rgb(c: [u8; 3]) -> Hsla {
    hex(pack(c))
}

/// `a` moved `t` of the way toward `b`.
fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let ch = |i: usize| (f32::from(a[i]) * (1.0 - t) + f32::from(b[i]) * t).round() as u8;
    [ch(0), ch(1), ch(2)]
}

/// The dominant colour as the surfaces see it: mixed far toward black or white so it only
/// ever leans a surface, never repaints it.
fn premixed(color: [u8; 3], light: bool) -> [u8; 3] {
    if light {
        mix(color, [255; 3], LIGHT_PREMIX)
    } else {
        mix(color, [0; 3], DARK_PREMIX)
    }
}

/// `color`, or the first mix of it toward `preferred`, black or white (in that order, in 1%
/// steps) that keeps `minimum` contrast on every one of `backgrounds`; else the best seen.
pub fn harden(
    color: [u8; 3],
    backgrounds: &[[u8; 3]],
    minimum: f32,
    preferred: Option<[u8; 3]>,
) -> [u8; 3] {
    let worst = |c: [u8; 3]| {
        backgrounds
            .iter()
            .map(|b| contrast_ratio(pack(c), pack(*b)))
            .fold(f32::INFINITY, f32::min)
    };
    if worst(color) >= minimum {
        return color;
    }
    let mut best = color;
    let mut best_contrast = worst(color);
    for target in preferred.into_iter().chain([[0; 3], [255; 3]]) {
        for step in 1..=100u8 {
            let candidate = mix(color, target, f32::from(step) / 100.0);
            let contrast = worst(candidate);
            if contrast > best_contrast {
                best = candidate;
                best_contrast = contrast;
            }
            if contrast >= minimum {
                return candidate;
            }
        }
    }
    best
}

/// A terminal scheme leaning toward the tint, its text kept readable on the new background.
pub fn tint_terminal(theme: &mut TerminalTheme, color: [u8; 3], light: bool) {
    let background = mix(
        rgb_of(theme.background),
        premixed(color, light),
        SURFACE_MIX,
    );
    theme.background = from_rgb(background).opacity(theme.background.a);
    let foreground = harden(rgb_of(theme.foreground), &[background], TEXT_CONTRAST, None);
    theme.foreground = from_rgb(foreground);
}

impl Theme {
    /// This palette taken toward the wallpaper's dominant `color`.
    pub fn tinted(mut self, color: [u8; 3]) -> Self {
        let tint = premixed(color, self.light);
        let lean = |c: Hsla| from_rgb(mix(rgb_of(c), tint, SURFACE_MIX)).opacity(c.a);
        self.shell = lean(self.shell);
        self.popup = lean(self.popup);
        self.terminal_background = lean(self.terminal_background);
        self.tint = Some(color);

        let surfaces = [
            rgb_of(self.shell),
            rgb_of(self.popup),
            rgb_of(self.terminal_background),
        ];
        let text = harden(rgb_of(self.text), &surfaces, TEXT_CONTRAST, None);
        let muted = harden(rgb_of(self.muted), &surfaces, TEXT_CONTRAST, Some(text));
        let faint = harden(rgb_of(self.faint), &surfaces, TEXT_CONTRAST, Some(text));
        (self.text, self.muted, self.faint) = (from_rgb(text), from_rgb(muted), from_rgb(faint));

        let accent = accent_for(color, surfaces[2]);
        self.accent = from_rgb(accent);
        self.border = self.accent.opacity(0.14);
        if self.light {
            // A white wash would vanish on a light surface.
            self.row_hover = self.accent.opacity(0.09);
            self.row_active = self.accent.opacity(0.15);
        } else {
            // Glass interactions lift toward white rather than laying a dark accent over it.
            self.row_hover = hsla(0.0, 0.0, 1.0, 0.09);
            self.row_active = hsla(0.0, 0.0, 1.0, 0.15);
        }
        self
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const WALLPAPERS: [[u8; 3]; 4] = [[0, 0, 0], [255, 255, 255], [255, 220, 20], [10, 40, 240]];

    fn palettes() -> [Theme; 2] {
        [Theme::zeron_dark(), Theme::zeron_light()]
    }

    /// zeron `wallpaper_colours_keep_text_readable_in_light_and_dark_modes`.
    #[test]
    fn wallpaper_colours_keep_text_readable_in_light_and_dark_modes() {
        for base in palettes() {
            for color in WALLPAPERS {
                let theme = base.tinted(color);
                for surface in [theme.shell, theme.popup, theme.terminal_background] {
                    for (name, text) in [
                        ("text", theme.text),
                        ("muted", theme.muted),
                        ("faint", theme.faint),
                    ] {
                        let ratio = contrast_ratio(pack(rgb_of(text)), pack(rgb_of(surface)));
                        assert!(
                            ratio >= 4.49,
                            "{name} light={} on {color:?}: {ratio}",
                            base.light
                        );
                    }
                }
            }
        }
    }

    /// The stock palettes already clear 4.5 under every wallpaper, so the test above cannot see a
    /// missing step. Here grey text is fine on the shell but not on a lifted card.
    #[test]
    fn every_text_colour_is_hardened_against_every_surface() {
        let mut base = Theme::zeron_dark();
        base.popup = hex(0x707070);
        (base.text, base.muted, base.faint) = (hex(0x9a9a9a), hex(0x9a9a9a), hex(0x9a9a9a));
        for color in WALLPAPERS {
            let theme = base.tinted(color);
            for (name, text) in [
                ("text", theme.text),
                ("muted", theme.muted),
                ("faint", theme.faint),
            ] {
                for surface in [theme.shell, theme.popup, theme.terminal_background] {
                    let ratio = contrast_ratio(pack(rgb_of(text)), pack(rgb_of(surface)));
                    assert!(ratio >= 4.49, "{name} on {color:?}: {ratio}");
                }
            }
        }
    }

    /// zeron `wallpaper_glass_interactions_lift_toward_white_in_both_appearances`; light mode
    /// differs on purpose, a white wash being invisible on a pale surface.
    #[test]
    fn glass_hover_lifts_toward_white_in_dark_and_uses_the_accent_in_light() {
        let dark = Theme::zeron_dark().tinted([20, 60, 140]);
        for wash in [dark.row_hover, dark.row_active] {
            assert_eq!((wash.l, wash.s), (1.0, 0.0));
            assert!(wash.a > 0.0 && wash.a < 1.0);
        }
        assert!(dark.row_active.a > dark.row_hover.a);
        let light = Theme::zeron_light().tinted([20, 60, 140]);
        assert!(light.row_hover.s > 0.0 && light.row_hover.l < 1.0);
        assert_eq!(light.row_hover.h, light.accent.h);
    }

    #[test]
    fn surfaces_lean_toward_the_picture_without_taking_its_colour() {
        let blue = [10, 40, 240];
        for base in palettes() {
            let theme = base.tinted(blue);
            for (before, after) in [
                (base.shell, theme.shell),
                (base.popup, theme.popup),
                (base.terminal_background, theme.terminal_background),
            ] {
                let (b, a) = (rgb_of(before), rgb_of(after));
                assert_ne!(a, b);
                for i in 0..3 {
                    assert!(a[i].abs_diff(b[i]) < 90, "{a:?} {b:?}");
                }
            }
            assert_eq!(theme.tint, Some(blue));
        }
        // Blue leans a dark surface toward blue, and a light one away from red and green.
        let dark = Theme::zeron_dark().tinted(blue);
        assert!(rgb_of(dark.shell)[2] > rgb_of(Theme::zeron_dark().shell)[2]);
        let light = Theme::zeron_light().tinted(blue);
        assert!(rgb_of(light.shell)[0] < rgb_of(Theme::zeron_light().shell)[0]);
    }

    #[test]
    fn accent_keeps_contrast_on_the_panel() {
        for base in palettes() {
            for color in WALLPAPERS {
                let theme = base.tinted(color);
                let ratio = contrast_ratio(
                    pack(rgb_of(theme.accent)),
                    pack(rgb_of(theme.terminal_background)),
                );
                assert!(ratio >= 3.0, "{color:?}: {ratio}");
            }
        }
    }

    #[test]
    fn a_tinted_scheme_keeps_its_text_readable() {
        let scheme = crate::themes::find("Dracula").unwrap();
        for base in palettes() {
            for color in WALLPAPERS {
                let term = base.tinted(color).terminal(13.0, Some(scheme));
                let ratio =
                    contrast_ratio(pack(rgb_of(term.foreground)), pack(rgb_of(term.background)));
                assert!(ratio >= 4.49, "{color:?}: {ratio}");
            }
        }
    }

    #[test]
    fn harden_leaves_readable_text_alone_and_moves_unreadable_text() {
        let dark = [[10, 10, 10]];
        assert_eq!(harden([240, 240, 240], &dark, 4.5, None), [240, 240, 240]);
        let moved = harden([40, 40, 40], &dark, 4.5, None);
        assert!(contrast_ratio(pack(moved), pack(dark[0])) >= 4.5);
        // Against black and white at once nothing reaches 4.5; it returns the best it saw.
        let both = [[0, 0, 0], [255, 255, 255]];
        let best = harden([128, 128, 128], &both, 4.5, None);
        let score = |c: [u8; 3]| {
            both.iter()
                .map(|b| contrast_ratio(pack(c), pack(*b)))
                .fold(f32::INFINITY, f32::min)
        };
        assert!(score(best) >= score([128; 3]));
    }
}
