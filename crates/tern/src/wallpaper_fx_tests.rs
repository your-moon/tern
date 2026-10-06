//! Behaviour of each wallpaper effect and of the contrast guard, beyond zeron's own tests.

use crate::wallpaper_fx::{Effect, Guard, render, safe_opacity};

fn solid(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    (0..w * h)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect()
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
fn opacity_is_monotonic_in_image_brightness() {
    let (text, bg) = (0xE8E8EA, 0x060606);
    let at = |v: u8| {
        safe_opacity(
            &solid(8, 8, [v, v, v]),
            8,
            8,
            Guard {
                text_rgb: text,
                background_rgb: bg,
                region: 1.0,
                min_contrast: 4.5,
                max_opacity: 1.0,
            },
        )
    };
    let (a, b, c) = (at(80), at(160), at(255));
    assert!(a >= b && b >= c, "{a} {b} {c}");
    assert!(c < a);
}

#[test]
fn safe_opacity_never_exceeds_the_cap_and_judges_the_bright_tail() {
    let (text, bg) = (0xE8E8EA, 0x060606);
    let capped = safe_opacity(
        &solid(8, 8, [0, 0, 0]),
        8,
        8,
        Guard {
            text_rgb: text,
            background_rgb: bg,
            region: 1.0,
            min_contrast: 4.5,
            max_opacity: 0.4,
        },
    );
    assert!((capped - 0.4).abs() < 1e-6);
    // A lone white pixel is below the 95th-percentile tail and must not count; four rows must.
    let whiten = |img: &mut [u8], rows: usize| {
        for p in img.as_chunks_mut::<4>().0.iter_mut().take(rows * 10) {
            p[..3].copy_from_slice(&[255, 255, 255]);
        }
    };
    let mut img = solid(10, 10, [0, 0, 0]);
    img[..3].copy_from_slice(&[255, 255, 255]);
    let few = safe_opacity(
        &img.clone(),
        10,
        10,
        Guard {
            text_rgb: text,
            background_rgb: bg,
            region: 1.0,
            min_contrast: 4.5,
            max_opacity: 1.0,
        },
    );
    whiten(&mut img, 4);
    let many = safe_opacity(
        &img,
        10,
        10,
        Guard {
            text_rgb: text,
            background_rgb: bg,
            region: 1.0,
            min_contrast: 4.5,
            max_opacity: 1.0,
        },
    );
    assert!((few - 1.0).abs() < 1e-6, "{few}");
    assert!(many < 0.9, "{many}");
}

#[test]
fn degenerate_input_gives_zero_opacity() {
    assert_eq!(
        safe_opacity(
            &[],
            0,
            0,
            Guard {
                text_rgb: 0xFF_FFFF,
                background_rgb: 0,
                region: 1.0,
                min_contrast: 4.5,
                max_opacity: 1.0
            }
        ),
        0.0
    );
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
