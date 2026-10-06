#![allow(dead_code)] // wired into the window in the next commit
//! The wallpaper store and pipeline: a picked file is copied into `<config>/wallpapers/`, the
//! last few picks are remembered, and an image with its effect applied is rendered once (off
//! the UI thread) and cached as a PNG that the window then draws.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use image::{DynamicImage, ImageFormat, RgbaImage};

use crate::wallpaper_colors;
use crate::wallpaper_fx::{self, Effect};

/// Recent wallpapers kept (zeron `HISTORY_LIMIT`).
pub const HISTORY_LIMIT: usize = 8;
/// Longest edge of the rendered artwork; larger sources are scaled down to it.
const MAX_EDGE: u32 = 1600;
/// Longest edge of the thumbnail the contrast guard samples.
const SAMPLE_EDGE: u32 = 160;
/// Rendered files kept in the cache; the rest are removed, oldest first.
const CACHE_KEEP: usize = 6;
/// Bumped when an effect's output changes, so stale cache files are not reused.
const CACHE_VERSION: u32 = 1;
const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

fn store(config: &Path) -> PathBuf {
    config.join("wallpapers")
}

fn cache(config: &Path) -> PathBuf {
    store(config).join("cache")
}

/// Copies `source` into the wallpaper folder, so the choice survives the original being moved
/// or deleted. The same bytes always land in the same file.
pub fn import(source: &Path, config: &Path) -> Result<PathBuf, String> {
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| EXTENSIONS.contains(&e.as_str()))
        .ok_or("Wallpapers must be PNG, JPEG or WebP images")?;
    let len = std::fs::metadata(source)
        .map_err(|e| format!("Cannot read the image: {e}"))?
        .len();
    if len > MAX_SOURCE_BYTES {
        return Err("That image is larger than 64 MB".into());
    }
    let bytes = std::fs::read(source).map_err(|e| format!("Cannot read the image: {e}"))?;
    if !matches!(
        image::guess_format(&bytes),
        Ok(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return Err("That file is not a PNG, JPEG or WebP image".into());
    }
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let stem: String = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("wallpaper")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(32)
        .collect();
    let dir = store(config);
    let target = dir.join(format!("{stem}-{:08x}.{ext}", hasher.finish() as u32));
    if !target.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {dir:?}: {e}"))?;
        std::fs::write(&target, bytes).map_err(|e| format!("Cannot copy the image: {e}"))?;
    }
    Ok(target)
}

/// `path` to the front of `history`, once, with the oldest dropped past [`HISTORY_LIMIT`].
pub fn remember(history: &mut Vec<String>, path: &str) {
    history.retain(|p| p != path);
    history.insert(0, path.to_owned());
    history.truncate(HISTORY_LIMIT);
}

/// Deletes copies in the wallpaper folder that are neither in `keep` nor the current one.
pub fn prune(config: &Path, keep: &[String]) {
    let Ok(entries) = std::fs::read_dir(store(config)) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let kept = keep
            .iter()
            .any(|k| Path::new(k).file_name() == path.file_name());
        if !kept {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// An image ready to draw.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// The rendered PNG in the cache.
    pub image: PathBuf,
    /// The wallpaper's dominant colour.
    pub accent: Option<[u8; 3]>,
    /// A small RGBA thumbnail of the rendered image, for the contrast guard.
    pub sample: Sample,
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The highest opacity the artwork may have so text in `text` over `surface` keeps `contrast`.
pub fn safe_opacity(sample: &Sample, text: u32, surface: u32, contrast: f32, cap: f32) -> f32 {
    wallpaper_fx::safe_opacity(
        &sample.rgba,
        sample.width,
        sample.height,
        wallpaper_fx::Guard {
            text_rgb: text,
            background_rgb: surface,
            region: 1.0,
            min_contrast: contrast,
            max_opacity: cap,
        },
    )
}

fn cache_name(source: &Path, len: u64, effect: Effect, light: bool) -> String {
    let mut hasher = DefaultHasher::new();
    (
        source.file_name(),
        len,
        effect as u8,
        light,
        MAX_EDGE,
        CACHE_VERSION,
    )
        .hash(&mut hasher);
    format!("{:016x}.png", hasher.finish())
}

/// Decodes `source`, applies `effect`, and caches the result. Heavy: call off the UI thread.
pub fn prepare(
    source: &Path,
    effect: Effect,
    light: bool,
    config: &Path,
) -> Result<Prepared, String> {
    let bytes = std::fs::read(source).map_err(|e| format!("Cannot read the wallpaper: {e}"))?;
    let decoded =
        image::load_from_memory(&bytes).map_err(|e| format!("Cannot decode the wallpaper: {e}"))?;
    let decoded = if decoded.width() > MAX_EDGE || decoded.height() > MAX_EDGE {
        decoded.thumbnail(MAX_EDGE, MAX_EDGE)
    } else {
        decoded
    };
    let accent =
        wallpaper_colors::extract(decoded.thumbnail(64, 64).to_rgba8().pixels().map(|p| p.0));
    let dir = cache(config);
    let path = dir.join(cache_name(source, bytes.len() as u64, effect, light));
    let rendered = match image::open(&path) {
        Ok(cached) => cached.to_rgba8(),
        Err(_) => {
            let rgba = decoded.to_rgba8();
            let (w, h) = rgba.dimensions();
            let out = wallpaper_fx::render(rgba.into_raw(), w, h, effect, light);
            let out = RgbaImage::from_raw(w, h, out).ok_or("Wallpaper effect lost pixels")?;
            std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {dir:?}: {e}"))?;
            out.save_with_format(&path, ImageFormat::Png)
                .map_err(|e| format!("Cannot cache the wallpaper: {e}"))?;
            trim_cache(&dir, &path);
            out
        }
    };
    let small = DynamicImage::ImageRgba8(rendered)
        .thumbnail(SAMPLE_EDGE, SAMPLE_EDGE)
        .to_rgba8();
    let (width, height) = small.dimensions();
    Ok(Prepared {
        image: path,
        accent,
        sample: Sample {
            rgba: small.into_raw(),
            width,
            height,
        },
    })
}

/// Keeps the newest [`CACHE_KEEP`] files, and never removes `current`.
fn trim_cache(dir: &Path, current: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in files.into_iter().skip(CACHE_KEEP) {
        if path != current {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "wallpaper_tests.rs"]
mod tests;
