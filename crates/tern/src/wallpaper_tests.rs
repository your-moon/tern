//! Tests for `wallpaper.rs`, kept in their own file to stay under the file-size ceiling.
#![allow(clippy::unwrap_used)]

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
            for file in [&p.image, &p.blurred, &p.image.with_extension("meta.json")] {
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
    assert_eq!(
        kept,
        CACHE_KEEP * 3,
        "a picture, its frosted copy and its colours"
    );
    assert!(!made[0].exists(), "the oldest was removed");
    assert!(
        !made[0].with_extension("meta.json").exists(),
        "its colours went with it"
    );
    assert!(made[made.len() - 1].with_extension("meta.json").exists());
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

fn assert_same(a: &Prepared, b: &Prepared) {
    assert_eq!(a.image, b.image);
    assert_eq!(a.blurred, b.blurred);
    assert_eq!(a.accent, b.accent);
    assert_eq!(a.backdrop, b.backdrop);
}

fn meta_of(p: &Prepared) -> PathBuf {
    p.image.with_extension("meta.json")
}

#[test]
fn the_cache_name_uses_the_file_length_the_bytes_would_have() {
    let dir = temp("name");
    let src = dir.join("w.png");
    write_png(&src, 33, 17);
    let len = std::fs::read(&src).unwrap().len() as u64;
    assert_eq!(len, std::fs::metadata(&src).unwrap().len());
    let p = prepare(&src, Effect::Dither, true, &dir).unwrap();
    assert_eq!(
        p.image.file_name().unwrap().to_str().unwrap(),
        cache_name(&src, len, Effect::Dither, true)
    );
}

#[test]
fn a_hit_returns_what_the_slow_path_returned() {
    for (i, effect) in [
        Effect::None,
        Effect::Scanlines,
        Effect::Ascii,
        Effect::Dither,
    ]
    .into_iter()
    .enumerate()
    {
        let dir = temp(&format!("hit-{i}"));
        let src = dir.join("w.png");
        if i % 2 == 0 {
            checkerboard(90 + i as u32, 50, 7).save(&src).unwrap();
        } else {
            write_png(&src, 60, 40 + i as u32);
        }
        let slow = prepare(&src, effect, i == 1, &dir).unwrap();
        assert!(meta_of(&slow).is_file());
        let hit = prepare(&src, effect, i == 1, &dir).unwrap();
        assert_same(&slow, &hit);
        std::fs::remove_file(meta_of(&slow)).unwrap();
        let again = prepare(&src, effect, i == 1, &dir).unwrap();
        assert_same(&slow, &again);
    }
}

#[test]
fn a_hit_decodes_nothing() {
    let dir = temp("nodecode");
    let src = dir.join("w.png");
    write_png(&src, 40, 30);
    let first = prepare(&src, Effect::Scanlines, false, &dir).unwrap();
    let len = std::fs::metadata(&src).unwrap().len() as usize;
    std::fs::write(&src, vec![0x5a; len]).unwrap();
    let stub: Vec<u8> = PNG_SIGNATURE.iter().chain(&PNG_IEND).copied().collect();
    std::fs::write(&first.image, &stub).unwrap();
    std::fs::write(&first.blurred, &stub).unwrap();
    assert!(image::open(&first.image).is_err());
    let hit = prepare(&src, Effect::Scanlines, false, &dir).unwrap();
    assert_same(&first, &hit);
    std::fs::remove_file(meta_of(&first)).unwrap();
    assert!(
        prepare(&src, Effect::Scanlines, false, &dir).is_err(),
        "without the colours the slow path runs, and cannot decode the garbage"
    );
}

#[test]
fn a_truncated_cached_picture_is_rendered_again() {
    for victim in ["image", "blurred"] {
        let dir = temp(&format!("trunc-{victim}"));
        let src = dir.join("w.png");
        write_png(&src, 40, 30);
        let first = prepare(&src, Effect::Scanlines, false, &dir).unwrap();
        let file = if victim == "image" {
            &first.image
        } else {
            &first.blurred
        };
        let bytes = std::fs::read(file).unwrap();
        std::fs::write(file, &bytes[..bytes.len() - 12]).unwrap();
        assert!(png_complete(&first.image) != (victim == "image"));
        let again = prepare(&src, Effect::Scanlines, false, &dir).unwrap();
        assert_same(&first, &again);
        assert_eq!(std::fs::read(file).unwrap(), bytes, "regenerated whole");
        assert!(png_complete(&again.image) && png_complete(&again.blurred));
    }
}

#[test]
fn a_missing_sidecar_is_made_again() {
    let dir = temp("nometa");
    let src = dir.join("w.png");
    write_png(&src, 40, 30);
    let first = prepare(&src, Effect::Ascii, false, &dir).unwrap();
    std::fs::remove_file(meta_of(&first)).unwrap();
    let again = prepare(&src, Effect::Ascii, false, &dir).unwrap();
    assert!(meta_of(&again).is_file());
    assert_same(&first, &again);
}

#[test]
fn a_corrupt_or_old_sidecar_is_ignored_and_replaced() {
    let dir = temp("badmeta");
    let src = dir.join("w.png");
    write_png(&src, 40, 30);
    let first = prepare(&src, Effect::Ascii, false, &dir).unwrap();
    let good = std::fs::read_to_string(meta_of(&first)).unwrap();
    let old = good.replace(
        &format!("\"version\":{META_VERSION}"),
        &format!("\"version\":{}", META_VERSION + 1),
    );
    assert_ne!(old, good, "the version is in the file");
    for bad in [old.as_str(), "{\"version\":", "", "[]"] {
        std::fs::write(meta_of(&first), bad).unwrap();
        let again = prepare(&src, Effect::Ascii, false, &dir).unwrap();
        assert_same(&first, &again);
        assert_eq!(std::fs::read_to_string(meta_of(&first)).unwrap(), good);
    }
}

#[test]
fn trimming_keeps_a_current_sidecar_and_drops_a_stray_one() {
    let dir = temp("stray");
    let src = dir.join("w.png");
    write_png(&src, 16, 16);
    let cache = dir.join("wallpapers/cache");
    let first = prepare(&src, Effect::None, false, &dir).unwrap();
    std::fs::write(cache.join("0000000000000000.meta.json"), "{}").unwrap();
    std::fs::File::options()
        .write(true)
        .open(cache.join("0000000000000000.meta.json"))
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
    for i in 0..CACHE_KEEP {
        std::fs::write(cache.join(format!("{:016x}.png", i + 1)), "x").unwrap();
    }
    let again = prepare(&src, Effect::None, false, &dir).unwrap();
    assert_same(&first, &again);
    assert!(meta_of(&again).is_file());
    assert!(!cache.join("0000000000000000.meta.json").exists());
}

/// The colours file is an extra: a cache folder that refuses it must not lose the wallpaper.
#[test]
fn an_unwritable_sidecar_does_not_fail_prepare() {
    let dir = temp("sidecar-unwritable");
    let src = dir.join("w.png");
    write_png(&src, 40, 30);
    let len = std::fs::metadata(&src).unwrap().len();
    let cache = cache(&dir);
    std::fs::create_dir_all(&cache).unwrap();
    // A directory where the file should go makes the write fail.
    let sidecar = cache
        .join(cache_name(&src, len, Effect::None, false))
        .with_extension("meta.json");
    std::fs::create_dir_all(&sidecar).unwrap();
    let p = prepare(&src, Effect::None, false, &dir);
    assert!(p.is_ok(), "{:?}", p.err());
}
