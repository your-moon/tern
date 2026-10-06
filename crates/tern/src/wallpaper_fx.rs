// Adapted from zeron crates/mobile/src/wallpaper.rs (artwork effects) (MIT).
// Adapted from zeron crates/ui/src/settings/wallpaper_colors.rs (extract) (MIT).
//! Wallpaper pixel work: artwork effects, the dominant colour of a picture, an accent derived
//! from it, and the WCAG contrast the theme hardening measures with.
//!
//! Pixels are straight RGBA8, row-major. An effect runs once per (image, effect, appearance)
//! off the UI thread and the result is cached as an image file.

use serde::{Deserialize, Serialize};

/// Artwork treatment (desktop `NewThreadBackgroundEffect`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    #[default]
    None,
    Dither,
    Ascii,
    Halftone,
    Scanlines,
}

fn luma(r: u8, g: u8, b: u8) -> u8 {
    // image-rs `to_luma8` (Rec. 709 weights), as the desktop samples it.
    ((2126 * r as u32 + 7152 * g as u32 + 722 * b as u32) / 10000) as u8
}

struct Source<'a> {
    w: u32,
    h: u32,
    rgba: &'a [u8],
}

impl Source<'_> {
    fn color(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    fn luma(&self, x: u32, y: u32) -> u8 {
        let [r, g, b, _] = self.color(x, y);
        luma(r, g, b)
    }
}

/// Apply `effect` to an RGBA image; `light` is the appearance it's drawn in
/// (effects print on white paper in light mode, black in dark).
pub fn render(rgba: Vec<u8>, width: u32, height: u32, effect: Effect, light: bool) -> Vec<u8> {
    if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
        return rgba;
    }
    let src = Source {
        w: width,
        h: height,
        rgba: &rgba,
    };
    match effect {
        Effect::None => rgba.clone(),
        Effect::Scanlines => scanlines(&src, light),
        Effect::Ascii => ascii(&src, light),
        Effect::Halftone => halftone(&src, light),
        Effect::Dither => dither(&src),
    }
}

fn scanlines(s: &Source, light: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.rgba.len());
    for y in 0..s.h {
        let gain = if y % 3 == 0 { 0.52 } else { 1.0 };
        for x in 0..s.w {
            let [r, g, b, a] = s.color(x, y);
            let ch = |v: u8| {
                if light {
                    (v as f32 + (255.0 - v as f32) * (1.0 - gain)) as u8
                } else {
                    (v as f32 * gain) as u8
                }
            };
            out.extend_from_slice(&[ch(r), ch(g), ch(b), a]);
        }
    }
    out
}

fn ascii(s: &Source, light: bool) -> Vec<u8> {
    // Five-column bitmap glyphs, one column/row of spacing (desktop table).
    const GLYPHS: [[u8; 7]; 10] = [
        [0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 4, 0],
        [0, 4, 0, 0, 4, 0, 0],
        [0, 0, 0, 14, 0, 0, 0],
        [0, 0, 14, 0, 14, 0, 0],
        [0, 4, 4, 31, 4, 4, 0],
        [0, 21, 14, 31, 14, 21, 0],
        [10, 10, 31, 10, 31, 10, 10],
        [17, 2, 4, 4, 8, 16, 17],
        [14, 17, 23, 21, 23, 16, 14],
    ];
    let mut out = Vec::with_capacity(s.rgba.len());
    for y in 0..s.h {
        for x in 0..s.w {
            let sx = (x / 6 * 6 + 3).min(s.w - 1);
            let sy = (y / 8 * 8 + 4).min(s.h - 1);
            let l = s.luma(sx, sy);
            let density = if light { 255 - l } else { l };
            let index = ((density as f32 / 255.0).sqrt() * 9.0) as usize;
            let ink = x % 6 < 5
                && y % 8 < 7
                && GLYPHS[index.min(9)][y as usize % 8] & (1 << (4 - x % 6)) != 0;
            let [r, g, b, a] = s.color(x, y);
            let [cr, cg, cb, _] = s.color(sx, sy);
            let paper = if light { 255.0 } else { 0.0 };
            let mix = |base: u8, glyph: u8| {
                (base as f32 * 0.60
                    + if ink {
                        glyph as f32 * 0.40
                    } else {
                        paper * 0.40
                    }) as u8
            };
            out.extend_from_slice(&[mix(r, cr), mix(g, cg), mix(b, cb), a]);
        }
    }
    out
}

fn halftone(s: &Source, light: bool) -> Vec<u8> {
    let paper = if light { 255u8 } else { 0 };
    let mut out = vec![0u8; s.rgba.len()];
    for px in out.as_chunks_mut::<4>().0.iter_mut() {
        px.copy_from_slice(&[paper, paper, paper, 255]);
    }
    for y in (0..s.h).step_by(4) {
        for x in (0..s.w).step_by(4) {
            let l = s.luma(x, y);
            let l = if light { 255 - l } else { l };
            let radius = 2.0 * (0.3 + 0.7 * (l as f32 / 255.0).sqrt());
            let [r, g, b, a] = s.color((x + 2).min(s.w - 1), (y + 2).min(s.h - 1));
            for dy in 0..4.min(s.h - y) {
                for dx in 0..4.min(s.w - x) {
                    let distance = ((dx as f32 - 1.5).powi(2) + (dy as f32 - 1.5).powi(2)).sqrt();
                    let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0) * a as f32 / 255.0;
                    let [sr, sg, sb, sa] = s.color(x + dx, y + dy);
                    let blend = |source: u8, dot: u8| {
                        (source as f32 * 0.60
                            + (dot as f32 * coverage + paper as f32 * (1.0 - coverage)) * 0.40)
                            as u8
                    };
                    let i = (((y + dy) * s.w + x + dx) * 4) as usize;
                    out[i..i + 4].copy_from_slice(&[blend(sr, r), blend(sg, g), blend(sb, b), sa]);
                }
            }
        }
    }
    out
}

fn dither(s: &Source) -> Vec<u8> {
    const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let mut out = vec![0u8; s.rgba.len()];
    for y in (0..s.h).step_by(2) {
        for x in (0..s.w).step_by(2) {
            let [r, g, b, a] = s.color((x + 1).min(s.w - 1), (y + 1).min(s.h - 1));
            let threshold = BAYER[y as usize / 2 % 4][x as usize / 2 % 4];
            let peak = r.max(g).max(b) as f32;
            let bright = peak / 255.0 > (threshold as f32 + 0.5) / 16.0;
            let gain = if bright { 255.0 / peak.max(1.0) } else { 0.08 };
            let c = [
                (r as f32 * gain).round() as u8,
                (g as f32 * gain).round() as u8,
                (b as f32 * gain).round() as u8,
                a,
            ];
            for dy in 0..2.min(s.h - y) {
                for dx in 0..2.min(s.w - x) {
                    let i = (((y + dy) * s.w + x + dx) * 4) as usize;
                    out[i..i + 4].copy_from_slice(&c);
                }
            }
        }
    }
    out
}

// MARK: - Contrast guard

fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn rel_luminance(rgb: u32) -> f32 {
    let (r, g, b) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// WCAG contrast ratio between two `0xRRGGBB` colours.
pub fn contrast_ratio(a_rgb: u32, b_rgb: u32) -> f32 {
    contrast(rel_luminance(a_rgb), rel_luminance(b_rgb))
}

fn contrast(a: f32, b: f32) -> f32 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// Contrast an accent keeps against the surface it sits on (WCAG non-text minimum).
pub const ACCENT_CONTRAST: f32 = 3.0;

/// Quantized dominant colour, favouring chromatic regions over neutral pixels. Sampling is
/// bounded by the caller; transparent pixels do not influence it.
pub fn extract(pixels: impl IntoIterator<Item = [u8; 4]>) -> Option<[u8; 3]> {
    let mut bins = vec![(0.0_f64, [0.0_f64; 3]); 4096];
    for [r, g, b, a] in pixels {
        if a < 128 {
            continue;
        }
        let high = f64::from(r.max(g).max(b));
        let low = f64::from(r.min(g).min(b));
        let saturation = (high - low) / high.max(1.0);
        let weight = (0.2 + saturation * saturation) * f64::from(a) / 255.0;
        let index =
            ((usize::from(r) >> 4) << 8) | ((usize::from(g) >> 4) << 4) | (usize::from(b) >> 4);
        let (count, channels) = &mut bins[index];
        *count += weight;
        for (sum, channel) in channels.iter_mut().zip([r, g, b]) {
            *sum += f64::from(channel) * weight;
        }
    }
    let (count, channels) = bins.into_iter().max_by(|a, b| a.0.total_cmp(&b.0))?;
    (count > 0.0).then(|| channels.map(|c| (c / count).round() as u8))
}

fn pack(c: [u8; 3]) -> u32 {
    u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])
}

/// `color` moved toward white (on a dark surface) or black (on a light one), in 5% steps, until
/// it keeps [`ACCENT_CONTRAST`] against `surface`. A colour that already does is returned as is.
pub fn accent_for(color: [u8; 3], surface: [u8; 3]) -> [u8; 3] {
    let surface_rgb = pack(surface);
    let dark_surface = contrast_ratio(surface_rgb, 0) < contrast_ratio(surface_rgb, 0xFF_FFFF);
    let target = if dark_surface { 255.0 } else { 0.0 };
    for step in 0..=20u8 {
        let t = f32::from(step) * 0.05;
        let mixed = color.map(|c| (f32::from(c) * (1.0 - t) + target * t).round() as u8);
        if contrast_ratio(pack(mixed), surface_rgb) >= ACCENT_CONTRAST {
            return mixed;
        }
    }
    [target as u8; 3]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        (0..w * h)
            .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
            .collect()
    }

    #[test]
    fn effects_keep_size_and_differ_from_source() {
        let src: Vec<u8> = (0..32 * 24)
            .flat_map(|i| [(i % 255) as u8, (i * 3 % 255) as u8, 200, 255])
            .collect();
        for effect in [
            Effect::Dither,
            Effect::Ascii,
            Effect::Halftone,
            Effect::Scanlines,
        ] {
            for light in [false, true] {
                let out = render(src.clone(), 32, 24, effect, light);
                assert_eq!(out.len(), src.len());
                assert_ne!(out, src, "{effect:?} light={light}");
            }
        }
        assert_eq!(render(src.clone(), 32, 24, Effect::None, false), src);
    }

    #[test]
    fn scanlines_match_desktop_gain() {
        let out = render(solid(3, 3, [200, 100, 50]), 3, 3, Effect::Scanlines, false);
        assert_eq!(&out[0..3], &[104, 52, 26]); // row 0: × 0.52
        assert_eq!(&out[12..15], &[200, 100, 50]); // row 1 untouched
    }

    #[test]
    fn extraction_ignores_transparency_and_favours_prominent_colour() {
        let mut pixels = vec![[90, 90, 90, 255]; 100];
        pixels.extend(vec![[30, 130, 220, 255]; 80]);
        pixels.extend(vec![[255, 0, 0, 0]; 1000]);
        assert_eq!(extract(pixels), Some([30, 130, 220]));
        assert_eq!(extract([[100, 100, 100, 255]; 10]), Some([100, 100, 100]));
        assert_eq!(extract([[255, 0, 0, 0]; 10]), None);
    }

    #[test]
    fn mostly_transparent_pixels_do_not_vote() {
        let mut pixels = vec![[255, 0, 0, 100]; 1000];
        pixels.extend(vec![[90, 90, 90, 255]; 10]);
        assert_eq!(extract(pixels), Some([90, 90, 90]));
    }

    #[test]
    fn a_smaller_saturated_area_beats_a_larger_grey_one() {
        // Weight is 0.2 for grey and 1.2 for pure blue: 30 blue pixels outweigh 100 grey.
        let mut pixels = vec![[128, 128, 128, 255]; 100];
        pixels.extend(vec![[0, 0, 255, 255]; 30]);
        assert_eq!(extract(pixels), Some([0, 0, 255]));
    }

    #[test]
    fn accent_keeps_contrast_on_dark_and_light_surfaces() {
        let colors = [
            [0, 0, 0],
            [255, 255, 255],
            [250, 220, 30],
            [20, 70, 200],
            [10, 10, 40],
            [200, 30, 30],
        ];
        for surface in [[9, 9, 9], [22, 24, 26], [243, 243, 245], [255, 255, 255]] {
            for color in colors {
                let accent = accent_for(color, surface);
                let ratio = contrast_ratio(pack(accent), pack(surface));
                assert!(ratio >= 3.0, "{color:?} on {surface:?}: {ratio}");
            }
        }
    }

    #[test]
    fn a_colour_that_is_already_readable_is_left_alone() {
        assert_eq!(accent_for([250, 220, 30], [9, 9, 9]), [250, 220, 30]);
    }

    #[test]
    fn a_dark_colour_is_lightened_on_a_dark_surface_and_darkened_on_a_light_one() {
        let on_dark = accent_for([10, 10, 40], [9, 9, 9]);
        assert!(on_dark.iter().map(|&c| u32::from(c)).sum::<u32>() > 60);
        let on_light = accent_for([250, 250, 220], [250, 250, 250]);
        assert!(on_light.iter().map(|&c| u32::from(c)).sum::<u32>() < 700);
    }

    fn lit(out: &[u8]) -> usize {
        out.as_chunks::<4>().0.iter().filter(|p| p[0] > 100).count()
    }

    #[test]
    fn dither_lights_a_share_of_pixels_that_follows_brightness() {
        let dark = render(solid(16, 16, [20, 20, 20]), 16, 16, Effect::Dither, false);
        let mid = render(
            solid(16, 16, [128, 128, 128]),
            16,
            16,
            Effect::Dither,
            false,
        );
        let bright = render(
            solid(16, 16, [250, 250, 250]),
            16,
            16,
            Effect::Dither,
            false,
        );
        assert!(lit(&dark) < lit(&mid), "{} {}", lit(&dark), lit(&mid));
        assert!(lit(&mid) < lit(&bright));
        // Mid grey lights about half of a Bayer tile, not none and not all.
        assert!((100..160).contains(&lit(&mid)), "{}", lit(&mid));
        // A lit pixel is lifted to full peak brightness.
        assert!(mid.as_chunks::<4>().0.iter().any(|p| p[0] == 255));
    }

    #[test]
    fn ascii_prints_glyphs_only_where_the_image_is_bright() {
        let black = solid(12, 16, [0, 0, 0]);
        assert_eq!(render(black.clone(), 12, 16, Effect::Ascii, false), black);
        let out = render(solid(12, 16, [255, 255, 255]), 12, 16, Effect::Ascii, false);
        let values: std::collections::HashSet<u8> =
            out.as_chunks::<4>().0.iter().map(|p| p[0]).collect();
        assert!(values.len() > 1, "glyph pixels and gaps differ");
    }

    #[test]
    fn halftone_dots_grow_with_brightness() {
        let dim = render(solid(16, 16, [30, 30, 30]), 16, 16, Effect::Halftone, false);
        let bright = render(
            solid(16, 16, [250, 250, 250]),
            16,
            16,
            Effect::Halftone,
            false,
        );
        let sum = |o: &[u8]| {
            o.as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from(p[0]))
                .sum::<u32>()
        };
        assert!(sum(&dim) < sum(&bright));
    }

    #[test]
    fn light_scanlines_lift_toward_white() {
        let out = render(solid(3, 3, [100, 100, 100]), 3, 3, Effect::Scanlines, true);
        assert!(out[0] > 100, "row 0 is lightened on paper-white");
        assert_eq!(out[12], 100);
    }

    #[test]
    fn short_buffer_is_returned_untouched() {
        let src = vec![1, 2, 3, 4];
        assert_eq!(render(src.clone(), 8, 8, Effect::Dither, false), src);
    }

    #[test]
    fn halftone_pixel_blends_source_and_dot_coverage() {
        // Dim source, so the dot is smaller than the cell and coverage is partial at (1, 1):
        // radius 0.878, distance 0.707, coverage 0.67; 10 * 0.6 + 10 * 0.67 * 0.4 = 8.7.
        let out = render(solid(8, 8, [10, 10, 10]), 8, 8, Effect::Halftone, false);
        let at = |x: usize, y: usize| out[(y * 8 + x) * 4];
        assert_eq!(at(1, 1), 8);
        // The cell's corner is outside the dot: source only.
        assert_eq!(at(0, 0), 6);
    }

    #[test]
    fn dither_follows_the_bayer_matrix() {
        // Mid grey against thresholds 0 and 8 of 16: the first 2x2 block is on, the next is off.
        let out = render(solid(8, 8, [128, 128, 128]), 8, 8, Effect::Dither, false);
        assert_eq!(out[0], 255);
        assert_eq!(out[2 * 4], 10);
    }

    #[test]
    fn ascii_densest_glyph_is_drawn_for_white() {
        // Row 0 of the densest glyph is `.###.`: columns 1 to 3 are ink (full), 0 and 4 paper.
        let out = render(solid(12, 16, [255, 255, 255]), 12, 16, Effect::Ascii, false);
        let row0: Vec<u8> = (0..6).map(|x| out[x * 4]).collect();
        assert_eq!(row0, [153, 255, 255, 255, 153, 153]);
    }
}
