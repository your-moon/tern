use std::path::{Path, PathBuf};

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tern-wallpaper-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A gradient, so no effect can leave it unchanged by accident.
fn write_png(path: &Path, w: u32, h: u32) {
    RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([(x * 6) as u8, (y * 8) as u8, 120, 255])
    })
    .save(path)
    .unwrap();
}

#[test]
fn import_copies_the_file_and_the_same_bytes_land_in_the_same_place() {
    let dir = temp("import");
    let src = dir.join("My Photo.png");
    write_png(&src, 8, 8);
    let a = import(&src, &dir).unwrap();
    assert!(a.starts_with(dir.join("wallpapers")));
    assert!(
        a.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("My-Photo-")
    );
    std::fs::remove_file(&src).unwrap();
    assert!(a.is_file(), "the copy outlives the original");
    // Same bytes under another name give another stem but importing the same file twice is a no-op.
    write_png(&src, 8, 8);
    assert_eq!(import(&src, &dir).unwrap(), a);
    assert_eq!(
        std::fs::read_dir(dir.join("wallpapers")).unwrap().count(),
        1
    );
}

#[test]
fn import_refuses_what_is_not_a_supported_image() {
    let dir = temp("refuse");
    let text = dir.join("a.txt");
    std::fs::write(&text, "hello").unwrap();
    assert!(import(&text, &dir).is_err());
    let fake = dir.join("fake.png");
    std::fs::write(&fake, "not a png").unwrap();
    assert!(import(&fake, &dir).is_err());
    assert!(import(&dir.join("missing.png"), &dir).is_err());
    assert!(!dir.join("wallpapers").exists(), "nothing was copied");
}

#[test]
fn history_is_newest_first_without_repeats_and_capped() {
    let mut h = Vec::new();
    for i in 0..12 {
        remember(&mut h, &format!("/w/{i}.png"));
    }
    assert_eq!(h.len(), HISTORY_LIMIT);
    assert_eq!(h[0], "/w/11.png");
    remember(&mut h, "/w/8.png");
    assert_eq!(h[0], "/w/8.png");
    assert_eq!(h.iter().filter(|p| *p == "/w/8.png").count(), 1);
    assert_eq!(h.len(), HISTORY_LIMIT);
    assert!(!h.contains(&"/w/3.png".to_owned()));
}

#[test]
fn prune_removes_only_copies_that_are_not_kept() {
    let dir = temp("prune");
    let store = dir.join("wallpapers");
    std::fs::create_dir_all(store.join("cache")).unwrap();
    for name in ["a.png", "b.png", "c.png"] {
        std::fs::write(store.join(name), "x").unwrap();
    }
    std::fs::write(store.join("cache/render.png"), "x").unwrap();
    prune(&dir, &[store.join("b.png").to_string_lossy().into_owned()]);
    assert!(!store.join("a.png").exists());
    assert!(store.join("b.png").exists());
    assert!(!store.join("c.png").exists());
    assert!(
        store.join("cache/render.png").exists(),
        "the cache is its own folder"
    );
}

#[test]
fn prepare_renders_once_then_reuses_the_cached_image() {
    let dir = temp("prepare");
    let src = dir.join("w.png");
    write_png(&src, 40, 30);
    let first = prepare(&src, Effect::Scanlines, false, &dir).unwrap();
    assert!(first.image.starts_with(dir.join("wallpapers/cache")));
    let out = image::open(&first.image).unwrap().to_rgba8();
    assert_eq!(out.dimensions(), (40, 30));
    let source = image::open(&src).unwrap().to_rgba8();
    assert_ne!(
        out.as_raw(),
        source.as_raw(),
        "scanlines changed the pixels"
    );
    assert!(first.accent.is_some());
    assert!(
        !first.backdrop.is_empty(),
        "the panel opacity is sized from it"
    );

    // Swap the cached file for a solid one: a second call that reads it back proves the
    // effect was not rendered again.
    RgbaImage::from_pixel(40, 30, image::Rgba([255, 255, 255, 255]))
        .save(&first.image)
        .unwrap();
    let second = prepare(&src, Effect::Scanlines, false, &dir).unwrap();
    assert_eq!(second.image, first.image);
    let kept = image::open(&second.image).unwrap().to_rgba8();
    assert!(kept.as_chunks::<4>().0.iter().all(|p| p[0] == 255));
}

#[test]
fn each_effect_and_appearance_has_its_own_cache_file() {
    let dir = temp("keys");
    let src = dir.join("w.png");
    write_png(&src, 24, 24);
    let mut seen = std::collections::HashSet::new();
    for effect in [
        Effect::None,
        Effect::Scanlines,
        Effect::Ascii,
        Effect::Halftone,
        Effect::Dither,
    ] {
        for light in [false, true] {
            let p = prepare(&src, effect, light, &dir).unwrap();
            assert!(
                seen.insert(p.image),
                "{effect:?} light={light} reused a file"
            );
        }
    }
}

#[test]
fn no_effect_keeps_the_pixels() {
    let dir = temp("none");
    let src = dir.join("w.png");
    write_png(&src, 20, 12);
    let p = prepare(&src, Effect::None, false, &dir).unwrap();
    let out = image::open(&p.image).unwrap().to_rgba8();
    assert_eq!(out.as_raw(), image::open(&src).unwrap().to_rgba8().as_raw());
}

#[test]
fn large_sources_are_scaled_down() {
    let dir = temp("large");
    let src = dir.join("big.png");
    RgbaImage::from_pixel(3200, 800, image::Rgba([10, 20, 30, 255]))
        .save(&src)
        .unwrap();
    let p = prepare(&src, Effect::None, false, &dir).unwrap();
    assert_eq!(image::open(&p.image).unwrap().width(), MAX_EDGE);
}

#[test]
fn the_cache_keeps_only_the_newest_files() {
    let dir = temp("trim");
    let src = dir.join("w.png");
    write_png(&src, 16, 16);
    let effects = [
        Effect::None,
        Effect::Scanlines,
        Effect::Ascii,
        Effect::Halftone,
        Effect::Dither,
    ];
    let mut made = Vec::new();
    for effect in effects {
        for light in [false, true] {
            let p = prepare(&src, effect, light, &dir).unwrap();
            // Date each file by when it was made, oldest first, a day apart.
            let when = std::time::UNIX_EPOCH
                + std::time::Duration::from_secs(86_400 * (made.len() as u64 + 1));
            for file in [&p.image, &p.blurred] {
                std::fs::File::options()
                    .write(true)
                    .open(file)
                    .unwrap()
                    .set_modified(when)
                    .unwrap();
            }
            made.push(p.image);
        }
    }
    let kept = std::fs::read_dir(dir.join("wallpapers/cache"))
        .unwrap()
        .count();
    assert_eq!(kept, CACHE_KEEP);
    assert!(!made[0].exists(), "the oldest was removed");
    assert!(made[made.len() - 1].exists(), "the newest was kept");
}

#[test]
fn a_broken_image_is_an_error_not_a_panic() {
    let dir = temp("broken");
    let src = dir.join("w.png");
    std::fs::write(&src, "nope").unwrap();
    assert!(prepare(&src, Effect::None, false, &dir).is_err());
    assert!(prepare(&dir.join("gone.png"), Effect::None, false, &dir).is_err());
}

#[test]
fn import_checks_the_bytes_not_just_the_name() {
    let dir = temp("magic");
    let sneaky = dir.join("sneaky.png");
    std::fs::write(&sneaky, b"GIF89a\x01\x00\x01\x00\x00\x00\x00;").unwrap();
    assert!(import(&sneaky, &dir).is_err());
}

#[test]
fn different_images_with_the_same_file_name_do_not_overwrite_each_other() {
    let dir = temp("clash");
    std::fs::create_dir_all(dir.join("one")).unwrap();
    std::fs::create_dir_all(dir.join("two")).unwrap();
    write_png(&dir.join("one/wall.png"), 8, 8);
    write_png(&dir.join("two/wall.png"), 9, 9);
    let a = import(&dir.join("one/wall.png"), &dir).unwrap();
    let b = import(&dir.join("two/wall.png"), &dir).unwrap();
    assert_ne!(a, b);
    assert_eq!(image::open(&a).unwrap().width(), 8);
    assert_eq!(image::open(&b).unwrap().width(), 9);
}

fn checkerboard(w: u32, h: u32, square: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let v = if (x / square + y / square).is_multiple_of(2) {
            255
        } else {
            0
        };
        image::Rgba([v, v, v, 255])
    })
}

fn dark_panel() -> crate::wallpaper_panel::Panel<'static> {
    crate::wallpaper_panel::Panel {
        surface: [0x0d, 0x0d, 0x0d],
        texts: &[([0xe8, 0xe8, 0xea], 4.5), ([0xa9, 0xa9, 0xae], 4.5)],
    }
}

#[test]
fn frosted_sampling_needs_less_panel_than_the_sharp_picture() {
    use crate::wallpaper_panel::{MAX_ALPHA, backdrop, panel_alpha};
    let sharp = checkerboard(320, 180, 10);
    let sharp_alpha = panel_alpha(&backdrop(sharp.as_raw()), &[dark_panel()]);
    let frost = frosted(&sharp);
    let frost_alpha = panel_alpha(&backdrop(frost.as_raw()), &[dark_panel()]);
    // The sharp board has pure white squares; blurred it is a mid grey everywhere.
    assert!(
        sharp_alpha > 0.75 && sharp_alpha <= MAX_ALPHA,
        "{sharp_alpha}"
    );
    assert!(
        frost_alpha < sharp_alpha - 0.05,
        "{frost_alpha} vs {sharp_alpha}"
    );
    let grey = frost.get_pixel(160, 90).0[0];
    assert!((100..160).contains(&grey), "{grey}");
}

#[test]
fn frosting_lifts_saturation_and_leaves_grey_and_alpha_alone() {
    let blue = RgbaImage::from_pixel(32, 32, image::Rgba([60, 90, 200, 255]));
    let out = frosted(&blue).get_pixel(16, 16).0;
    assert!(out[2] > 200 && out[0] < 60, "{out:?}");
    assert_eq!(out[3], 255);
    let grey = RgbaImage::from_pixel(32, 32, image::Rgba([120, 120, 120, 255]));
    let out = frosted(&grey).get_pixel(16, 16).0;
    assert_eq!(out, [120, 120, 120, 255]);
    let faint = RgbaImage::from_pixel(32, 32, image::Rgba([60, 90, 200, 140]));
    assert_eq!(frosted(&faint).get_pixel(16, 16).0[3], 140);
}

#[test]
fn prepare_caches_a_frosted_copy_next_to_the_picture() {
    let dir = temp("frost");
    let src = dir.join("board.png");
    checkerboard(200, 120, 10).save(&src).unwrap();
    let p = prepare(&src, Effect::None, false, &dir).unwrap();
    assert_ne!(p.blurred, p.image);
    assert_eq!(p.blurred.parent(), p.image.parent());
    let blurred = image::open(&p.blurred).unwrap().to_rgba8();
    let mid = blurred
        .get_pixel(blurred.width() / 2, blurred.height() / 2)
        .0[0];
    assert!((90..170).contains(&mid), "{mid}");
    // The panels are sized from the frosted copy: no white squares left in the sample.
    assert!(p.backdrop.iter().all(|c| c[0] < 230), "{:?}", p.backdrop);
}
