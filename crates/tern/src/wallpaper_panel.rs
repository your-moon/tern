//! How opaque the panels over a full-window wallpaper must be. The picture shows at full
//! strength in the gaps and through each panel at `1 - alpha`; instead of dimming it, the alpha
//! is the smallest one at which every text colour keeps its contrast over the picture's
//! brightest and darkest regions, so a dark picture on a dark theme barely needs a panel and a
//! white one needs nearly opaque ones.

use crate::wallpaper_fx::contrast_ratio;

/// The least and most opaque a panel gets: below the floor text starts to swim on any picture,
/// above the cap the picture is hardly there.
pub const MIN_ALPHA: f32 = 0.55;
pub const MAX_ALPHA: f32 = 0.92;
/// Colours sampled from the picture, darkest to brightest.
const SAMPLES: usize = 21;
const STEP: f32 = 0.01;

/// A panel surface and the text colours that are drawn on it.
pub struct Panel<'a> {
    pub surface: [u8; 3],
    pub texts: &'a [[u8; 3]],
}

fn pack(c: [u8; 3]) -> u32 {
    u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])
}

/// [`SAMPLES`] colours from darkest to brightest, taken at even steps between the 2nd and 98th
/// percentile of the picture's brightness, so a speck of white does not decide the panels.
pub fn backdrop(rgba: &[u8]) -> Vec<[u8; 3]> {
    let mut pixels: Vec<[u8; 3]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] >= 128)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    // Contrast against black rises with luminance, which is all the ordering needs.
    pixels.sort_by(|a, b| contrast_ratio(pack(*a), 0).total_cmp(&contrast_ratio(pack(*b), 0)));
    if pixels.is_empty() {
        return Vec::new();
    }
    (0..SAMPLES)
        .map(|i| {
            let p = 0.02 + 0.96 * i as f32 / (SAMPLES - 1) as f32;
            pixels[((pixels.len() - 1) as f32 * p).round() as usize]
        })
        .collect()
}

/// `surface` laid over `under` at `alpha`, in gamma-encoded sRGB as the screen composites it.
fn over(surface: [u8; 3], under: [u8; 3], alpha: f32) -> [u8; 3] {
    let ch = |i: usize| {
        (f32::from(surface[i]) * alpha + f32::from(under[i]) * (1.0 - alpha)).round() as u8
    };
    [ch(0), ch(1), ch(2)]
}

/// Whether every text keeps `minimum` contrast on its panel over every sampled picture colour.
fn holds(backdrop: &[[u8; 3]], panels: &[Panel], alpha: f32, minimum: f32) -> bool {
    panels.iter().all(|panel| {
        backdrop.iter().all(|under| {
            let seen = pack(over(panel.surface, *under, alpha));
            panel
                .texts
                .iter()
                .all(|text| contrast_ratio(pack(*text), seen) >= minimum)
        })
    })
}

/// The smallest alpha in [[`MIN_ALPHA`], [`MAX_ALPHA`]] at which text holds `minimum` contrast
/// on every panel over the picture; the cap when none does (or the picture has no pixels).
pub fn panel_alpha(backdrop: &[[u8; 3]], panels: &[Panel], minimum: f32) -> f32 {
    if backdrop.is_empty() {
        return MAX_ALPHA;
    }
    let steps = ((MAX_ALPHA - MIN_ALPHA) / STEP).round() as u32;
    (0..=steps)
        .map(|i| MIN_ALPHA + STEP * i as f32)
        .find(|a| holds(backdrop, panels, *a, minimum))
        .unwrap_or(MAX_ALPHA)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(rgb: [u8; 3]) -> Vec<[u8; 3]> {
        vec![rgb; SAMPLES]
    }

    /// zeron dark: surface and the text, muted and faint colours drawn on it.
    const DARK: Panel = Panel {
        surface: [0x0d, 0x0d, 0x0d],
        texts: &[[0xe8, 0xe8, 0xea], [0xa9, 0xa9, 0xae], [0x85, 0x85, 0x8a]],
    };
    const DARK_TEXT_AND_MUTED: Panel = Panel {
        surface: [0x0d, 0x0d, 0x0d],
        texts: &[[0xe8, 0xe8, 0xea], [0xa9, 0xa9, 0xae]],
    };
    const LIGHT: Panel = Panel {
        surface: [0xf3, 0xf3, 0xf5],
        texts: &[[0x30, 0x30, 0x35], [0x62, 0x62, 0x6a]],
    };

    #[test]
    fn a_white_picture_needs_the_most_opaque_panels() {
        assert_eq!(panel_alpha(&solid([255; 3]), &[DARK], 4.5), MAX_ALPHA);
    }

    #[test]
    fn a_black_picture_on_a_dark_theme_needs_only_the_floor() {
        assert_eq!(panel_alpha(&solid([0; 3]), &[DARK], 4.5), MIN_ALPHA);
    }

    #[test]
    fn light_grey_lands_between_and_is_the_smallest_alpha_that_holds() {
        let grey = solid([200; 3]);
        let panels = [DARK_TEXT_AND_MUTED];
        let a = panel_alpha(&grey, &panels, 4.5);
        assert!(a > MIN_ALPHA + 0.1 && a < MAX_ALPHA, "{a}");
        assert!(holds(&grey, &panels, a, 4.5));
        assert!(!holds(&grey, &panels, a - STEP, 4.5), "{a} is not minimal");
    }

    #[test]
    fn yellow_needs_dark_panels_more_than_light_ones() {
        let yellow = solid([255, 220, 20]);
        let dark = panel_alpha(&yellow, &[DARK], 4.5);
        let light = panel_alpha(&yellow, &[LIGHT], 4.5);
        assert!(dark >= 0.8, "{dark}");
        assert_eq!(light, MIN_ALPHA);
        assert!(dark > light);
    }

    #[test]
    fn the_brightest_and_the_darkest_region_both_count() {
        // Half black, half white: the white half decides, as for an all-white picture.
        let mut split = solid([0; 3]);
        for c in split.iter_mut().skip(SAMPLES / 2) {
            *c = [255; 3];
        }
        let all_white = panel_alpha(&solid([255; 3]), &[DARK_TEXT_AND_MUTED], 4.5);
        assert_eq!(panel_alpha(&split, &[DARK_TEXT_AND_MUTED], 4.5), all_white);
        // And on a light theme the dark half is the threat.
        let light_text = panel_alpha(&solid([0; 3]), &[LIGHT], 4.5);
        assert_eq!(
            panel_alpha(&split, &[LIGHT], 4.5),
            light_text.max(MIN_ALPHA)
        );
    }

    #[test]
    fn every_panel_must_hold_not_only_the_first() {
        // Black picture: the dark panel needs only the floor, the light one more.
        let black = solid([0; 3]);
        let panels = [DARK_TEXT_AND_MUTED, LIGHT];
        let both = panel_alpha(&black, &panels, 4.5);
        assert_eq!(panel_alpha(&black, &[DARK_TEXT_AND_MUTED], 4.5), MIN_ALPHA);
        assert!(both > MIN_ALPHA, "{both}");
        assert!(holds(&black, &panels, both, 4.5));
    }

    #[test]
    fn no_pixels_means_the_cap() {
        assert_eq!(panel_alpha(&[], &[DARK], 4.5), MAX_ALPHA);
    }

    #[test]
    fn backdrop_runs_dark_to_bright_and_ignores_a_speck() {
        let mut px = Vec::new();
        for i in 0..100u32 {
            let v = (i * 2) as u8;
            px.extend([v, v, v, 255]);
        }
        px.extend([255, 255, 255, 255]); // a lone bright pixel past the 98th percentile
        px.extend([9, 9, 9, 0]); // transparent: not a pixel
        let b = backdrop(&px);
        assert_eq!(b.len(), SAMPLES);
        assert!(b.windows(2).all(|w| w[0][0] <= w[1][0]));
        assert!(b[0][0] < 10 && b[SAMPLES - 1][0] < 255);
        assert!(backdrop(&[]).is_empty());
    }
}
