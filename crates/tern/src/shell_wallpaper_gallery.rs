//! The Wallpaper card's gallery: a wrapping grid of thumbnail tiles — none, the built-in
//! pictures, the recent ones, and a "+" that opens the file picker. Thumbnails are made once
//! off the UI thread and cached on disk; until one is ready its tile is a neutral placeholder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext as _, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, StyledImage as _, div, hsla, px,
};

use super::Shell;
use crate::hover::Accessible as _;
use crate::hover::HoverFade as _;
use crate::theme::Theme;
use crate::wallpaper_gallery::{self as data, BUILTINS, Builtin};

/// A tile is 16:10, in whole points so it is whole pixels at 1x and 2x.
const TILE_W: f32 = 120.0;
const TILE_H: f32 = 75.0;
const TILE_GAP: f32 = 8.0;
const RING: f32 = 2.0;
const TILE_RADIUS: f32 = 8.0;

enum Thumb {
    Pending,
    Ready(PathBuf),
    Failed,
}

/// Thumbnails by tile key: a built-in's `builtin:<id>`, or a recent picture's path.
#[derive(Default)]
pub(super) struct Gallery {
    thumbs: HashMap<String, Thumb>,
}

fn builtin_key(b: &Builtin) -> String {
    format!("builtin:{}", b.id)
}

/// What a tile shows and does.
enum Tile {
    None,
    Builtin(&'static Builtin),
    Recent(String),
    Add,
}

impl Shell {
    /// Recent pictures that are still on disk, newest first.
    fn recent_pictures(&self) -> impl Iterator<Item = &String> {
        self.settings
            .wallpaper_history
            .iter()
            .filter(|p| Path::new(p).is_file())
    }

    /// Starts a thumbnail for every tile that has none yet. Called every frame while Settings is
    /// open; a map lookup per tile when nothing is missing.
    pub(super) fn sync_thumbs(&mut self, cx: &mut Context<Self>) {
        if self.settings_page.is_none() {
            return;
        }
        let Some(config) = crate::settings::dir() else {
            return;
        };
        let mut missing: Vec<(String, Option<&'static Builtin>)> =
            BUILTINS.iter().map(|b| (builtin_key(b), Some(b))).collect();
        missing.extend(self.recent_pictures().map(|p| (p.clone(), None)));
        missing.retain(|(key, _)| !self.wp.gallery.thumbs.contains_key(key));
        for (key, builtin) in missing {
            self.wp.gallery.thumbs.insert(key.clone(), Thumb::Pending);
            let config = config.clone();
            let job_key = key.clone();
            let task = cx.background_spawn(async move {
                match builtin {
                    Some(b) => data::builtin_thumbnail(&config, b),
                    None => data::file_thumbnail(&config, Path::new(&job_key)),
                }
            });
            cx.spawn(async move |this, cx| {
                let made = task.await;
                this.update(cx, |s, cx| {
                    let thumb = made.map_or(Thumb::Failed, Thumb::Ready);
                    s.wp.gallery.thumbs.insert(key, thumb);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// Writes the built-in into tern's folder (once) off the UI thread, then shows it. It is
    /// not remembered: it is always in the gallery.
    fn pick_builtin(&mut self, b: &'static Builtin, cx: &mut Context<Self>) {
        let Some(config) = crate::settings::dir() else {
            return;
        };
        let task = cx.background_spawn(async move { data::install_builtin(&config, b) });
        cx.spawn(async move |this, cx| {
            let written = task.await;
            this.update(cx, |s, cx| match written {
                Ok(path) => s.show_builtin(path.to_string_lossy().into_owned(), cx),
                Err(error) => {
                    s.wp.error = Some(error);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn tile_selected(&self, tile: &Tile) -> bool {
        let current = self.settings.wallpaper.as_deref();
        match tile {
            Tile::None => current.is_none(),
            Tile::Builtin(b) => {
                let config = crate::settings::dir();
                current.is_some_and(|c| {
                    config.is_some_and(|dir| Path::new(c) == data::builtin_path(&dir, b))
                })
            }
            Tile::Recent(path) => current == Some(path.as_str()),
            Tile::Add => false,
        }
    }

    fn tile(&self, index: usize, tile: Tile, cx: &mut Context<Self>) -> impl IntoElement {
        let t: Theme = self.theme;
        let selected = self.tile_selected(&tile);
        let (title, label): (SharedString, Option<&str>) = match &tile {
            Tile::None => ("No wallpaper".into(), Some("None")),
            Tile::Builtin(b) => (b.title.into(), None),
            Tile::Recent(path) => (
                Path::new(path)
                    .file_name()
                    .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned())
                    .into(),
                None,
            ),
            Tile::Add => ("Choose an image…".into(), Some("+")),
        };
        let thumb = match &tile {
            Tile::Builtin(b) => self.wp.gallery.thumbs.get(&builtin_key(b)),
            Tile::Recent(path) => self.wp.gallery.thumbs.get(path),
            _ => None,
        };
        let inner = TILE_RADIUS - RING;
        let hover_key = format!("wallpaper-tile-{index}");
        let click = cx.listener(move |s, _, _, cx| match &tile {
            Tile::None => s.clear_wallpaper(cx),
            Tile::Builtin(b) => s.pick_builtin(b, cx),
            Tile::Recent(path) => s.set_wallpaper(path.clone(), cx),
            Tile::Add => s.pick_wallpaper(cx),
        });
        div()
            .relative()
            .flex_none()
            .w(px(TILE_W))
            .h(px(TILE_H))
            .rounded(px(TILE_RADIUS))
            .border_2()
            .border_color(if selected {
                t.accent
            } else {
                hsla(0., 0., 0., 0.)
            })
            .child(
                // The placeholder under (and, until it loads, instead of) the thumbnail.
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(inner))
                    .bg(t.ink(0.06))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(label, |el, text| {
                        el.text_size(px(if text == "+" { 22. } else { 12.5 }))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.muted)
                            .child(text)
                    }),
            )
            .when_some(
                match thumb {
                    Some(Thumb::Ready(path)) => Some(path.clone()),
                    _ => None,
                },
                |el, path| {
                    el.child(
                        gpui::img(path)
                            .absolute()
                            .size_full()
                            .rounded(px(inner))
                            .object_fit(gpui::ObjectFit::Cover),
                    )
                },
            )
            .child(
                // The whole tile is the button; it lightens under the pointer.
                div()
                    .id(("wallpaper-tile", index))
                    .absolute()
                    .size_full()
                    .rounded(px(inner))
                    .cursor_pointer()
                    .hover_fade(hover_key, hsla(0., 0., 1., 0.), hsla(0., 0., 1., 0.14))
                    .icon_button(title, &t)
                    .on_click(click),
            )
    }

    /// The card's first block: what is shown now (or why it could not be), and the tiles.
    pub(super) fn wallpaper_gallery(&self, status: String, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let mut grid = div().flex().flex_wrap().gap(px(TILE_GAP)).mt(px(12.));
        let mut tiles = vec![Tile::None];
        tiles.extend(BUILTINS.iter().map(Tile::Builtin));
        tiles.extend(self.recent_pictures().cloned().map(Tile::Recent));
        tiles.push(Tile::Add);
        for (i, tile) in tiles.into_iter().enumerate() {
            grid = grid.child(self.tile(i, tile, cx));
        }
        div()
            .mx(px(16.))
            .py(px(14.))
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(17.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.text)
                    .child("Image"),
            )
            .child(
                div()
                    .mt(px(1.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .child(status),
            )
            .child(grid)
    }
}
