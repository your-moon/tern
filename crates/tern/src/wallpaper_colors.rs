// Adapted from zeron crates/ui/src/settings/wallpaper_colors.rs (extract) (MIT).
//! The wallpaper's dominant colour, and an accent derived from it that stays readable.

#![allow(dead_code)] // used from the wallpaper pipeline in the next commit

use crate::wallpaper_fx::contrast_ratio;

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
                assert!(
                    ratio >= 3.0,
                    "{color:?} on {surface:?}: {ratio}"
                );
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
}
