//! The wallpaper gallery's data: the pictures built into the binary, how one is written out as
//! an ordinary wallpaper file, and the small cached thumbnails the tiles draw instead of the
//! full image. Everything here is plain file work for a background thread.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};

use crate::settings::Settings;

/// A wallpaper that ships inside the binary.
pub struct Builtin {
    pub id: &'static str,
    pub title: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! builtin {
    ($id:literal, $title:literal) => {
        Builtin {
            id: $id,
            title: $title,
            bytes: include_bytes!(concat!("../assets/wallpapers/", $id, ".jpg")),
        }
    };
}

/// Public-domain works from The Met Open Access (credited in NOTICE.md).
pub static BUILTINS: [Builtin; 7] = [
    builtin!("great-wave", "The Great Wave — Hokusai"),
    builtin!("red-fuji", "Red Fuji — Hokusai"),
    builtin!("kanbara-snow", "Evening Snow at Kanbara — Hiroshige"),
    builtin!("shono-rain", "Sudden Shower at Shōno — Hiroshige"),
    builtin!("heart-of-the-andes", "Heart of the Andes — Church"),
    builtin!("wheat-field", "Wheat Field with Cypresses — van Gogh"),
    builtin!(
        "fontainebleau-morning",
        "Morning in the Forest of Fontainebleau — Rousseau"
    ),
];

/// Thumbnail size in pixels: a 120×75 pt tile at 2x.
pub const THUMB_W: u32 = 240;
pub const THUMB_H: u32 = 150;
/// Bumped when the thumbnail's look changes, so stale files are not reused.
const THUMB_VERSION: u32 = 1;

fn builtin_dir(config: &Path) -> PathBuf {
    crate::wallpaper::store(config).join("builtin")
}

/// Where `b` is (or will be) written as a wallpaper file.
pub fn builtin_path(config: &Path, b: &Builtin) -> PathBuf {
    builtin_dir(config).join(format!("{}.jpg", b.id))
}

/// Writes `b` into `<config>/wallpapers/builtin/` unless a file of the same length is already
/// there, and returns its path. A built-in is a fixed set of bytes, so length is enough.
pub fn install_builtin(config: &Path, b: &Builtin) -> Result<PathBuf, String> {
    let target = builtin_path(config, b);
    if std::fs::metadata(&target).is_ok_and(|m| m.len() == b.bytes.len() as u64) {
        return Ok(target);
    }
    let dir = builtin_dir(config);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {dir:?}: {e}"))?;
    std::fs::write(&target, b.bytes).map_err(|e| format!("Cannot write the wallpaper: {e}"))?;
    Ok(target)
}

/// A picked file, remembered in the history like any other.
pub fn use_picked(st: &mut Settings, path: String) {
    crate::wallpaper::remember(&mut st.wallpaper_history, &path);
    st.wallpaper = Some(path);
}

/// A built-in is always available again, so it never takes a place in the history.
pub fn use_builtin(st: &mut Settings, path: String) {
    st.wallpaper = Some(path);
}

/// The cache file name for the thumbnail of `id` (a built-in's id or a file's path) at `len`
/// bytes: another file, or the same one with other contents, gets its own thumbnail.
pub fn thumb_name(id: &str, len: u64) -> String {
    let mut hasher = DefaultHasher::new();
    (id, len, THUMB_W, THUMB_H, THUMB_VERSION).hash(&mut hasher);
    format!("{:016x}.jpg", hasher.finish())
}

fn thumb_dir(config: &Path) -> PathBuf {
    crate::wallpaper::store(config).join("thumbs")
}

/// The thumbnail of `id`, made from `load()`'s bytes once and reused from then on. Heavy: call
/// off the UI thread.
pub fn thumbnail(
    config: &Path,
    id: &str,
    len: u64,
    load: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<PathBuf, String> {
    let path = thumb_dir(config).join(thumb_name(id, len));
    if path.is_file() {
        return Ok(path);
    }
    let bytes = load()?;
    let full =
        image::load_from_memory(&bytes).map_err(|e| format!("Cannot decode the wallpaper: {e}"))?;
    let thumb = DynamicImage::ImageRgb8(
        full.resize_to_fill(THUMB_W, THUMB_H, FilterType::Triangle)
            .to_rgb8(),
    );
    let dir = thumb_dir(config);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {dir:?}: {e}"))?;
    thumb
        .save_with_format(&path, ImageFormat::Jpeg)
        .map_err(|e| format!("Cannot save the thumbnail: {e}"))?;
    Ok(path)
}

/// The thumbnail of a built-in, straight from the embedded bytes.
pub fn builtin_thumbnail(config: &Path, b: &Builtin) -> Result<PathBuf, String> {
    thumbnail(
        config,
        &format!("builtin:{}", b.id),
        b.bytes.len() as u64,
        || Ok(b.bytes.to_vec()),
    )
}

/// The thumbnail of a wallpaper file.
pub fn file_thumbnail(config: &Path, source: &Path) -> Result<PathBuf, String> {
    let len = std::fs::metadata(source)
        .map_err(|e| format!("Cannot read the image: {e}"))?
        .len();
    thumbnail(config, &source.to_string_lossy(), len, || {
        std::fs::read(source).map_err(|e| format!("Cannot read the image: {e}"))
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tern-gallery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img =
            image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 3) as u8, (y * 5) as u8, 90]));
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
}
