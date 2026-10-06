use std::collections::HashSet;

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tern-gallery-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 3) as u8, (y * 5) as u8, 90]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn builtins_are_seven_unique_jpegs() {
    assert_eq!(BUILTINS.len(), 7);
    let ids: HashSet<_> = BUILTINS.iter().map(|b| b.id).collect();
    assert_eq!(ids.len(), 7);
    let titles: HashSet<_> = BUILTINS.iter().map(|b| b.title).collect();
    assert_eq!(titles.len(), 7);
    for b in &BUILTINS {
        assert_eq!(&b.bytes[..3], &[0xff, 0xd8, 0xff], "{} is not a JPEG", b.id);
        assert_eq!(
            &b.bytes[b.bytes.len() - 2..],
            &[0xff, 0xd9],
            "{} is cut",
            b.id
        );
        assert_eq!(image::guess_format(b.bytes).unwrap(), ImageFormat::Jpeg);
    }
}

#[test]
fn thumbnail_names_follow_the_source_and_its_length() {
    let a = thumb_name("/w/a.png", 1000);
    assert_eq!(a, thumb_name("/w/a.png", 1000));
    assert_ne!(a, thumb_name("/w/a.png", 1001), "length is part of the key");
    assert_ne!(a, thumb_name("/w/b.png", 1000), "so is the source");
    assert_ne!(
        thumb_name("builtin:red-fuji", 5),
        thumb_name("builtin:great-wave", 5)
    );
}

#[test]
fn installing_a_builtin_is_idempotent_and_repairs_a_short_file() {
    let dir = temp("install");
    let b = &BUILTINS[0];
    let path = install_builtin(&dir, b).unwrap();
    assert_eq!(path, dir.join("wallpapers/builtin/great-wave.jpg"));
    assert_eq!(std::fs::read(&path).unwrap(), b.bytes);
    // Same length already there: left alone (a marker of the same length survives)...
    let marker = vec![7u8; b.bytes.len()];
    std::fs::write(&path, &marker).unwrap();
    assert_eq!(install_builtin(&dir, b).unwrap(), path);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        marker,
        "rewritten needlessly"
    );
    // ...while a truncated one is written again.
    std::fs::write(&path, b"short").unwrap();
    install_builtin(&dir, b).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b.bytes);
}

#[test]
fn a_thumbnail_is_made_once_at_tile_size_and_a_new_length_makes_another() {
    let dir = temp("thumb");
    let src = png_bytes(64, 64); // square source: the thumbnail is cropped to 16:10
    let first = thumbnail(&dir, "/w/a.png", src.len() as u64, || Ok(src.clone())).unwrap();
    let img = image::open(&first).unwrap();
    assert_eq!((img.width(), img.height()), (THUMB_W, THUMB_H));
    // Cached: the loader is not asked again.
    let again = thumbnail(
        &dir,
        "/w/a.png",
        src.len() as u64,
        || Err("reloaded".into()),
    )
    .unwrap();
    assert_eq!(again, first);
    let longer = thumbnail(&dir, "/w/a.png", src.len() as u64 + 1, || Ok(src.clone())).unwrap();
    assert_ne!(longer, first);
    assert!(thumbnail(&dir, "/w/bad", 3, || Ok(b"not an image".to_vec())).is_err());
}

#[test]
fn every_builtin_makes_a_thumbnail() {
    let dir = temp("builtin-thumbs");
    let paths: HashSet<_> = BUILTINS
        .iter()
        .map(|b| builtin_thumbnail(&dir, b).unwrap())
        .collect();
    assert_eq!(paths.len(), 7);
}

#[test]
fn builtins_stay_out_of_the_history_and_picked_files_go_in() {
    let mut st = Settings {
        wallpaper_history: vec!["/w/old.png".into()],
        ..Settings::default()
    };
    use_builtin(&mut st, "/c/wallpapers/builtin/red-fuji.jpg".into());
    assert_eq!(
        st.wallpaper.as_deref(),
        Some("/c/wallpapers/builtin/red-fuji.jpg")
    );
    assert_eq!(st.wallpaper_history, ["/w/old.png"]);
    use_picked(&mut st, "/w/new.png".into());
    assert_eq!(st.wallpaper_history, ["/w/new.png", "/w/old.png"]);
}
