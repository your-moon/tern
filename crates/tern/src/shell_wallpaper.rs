// CubicBezier and the crossfade timing are zeron's (crates/ui/src/motion.rs WALLPAPER_CROSSFADE,
// MIT); the history and contrast guard follow crates/ui/src/settings/wallpaper.rs (MIT).
//! The wallpaper: loading it off the UI thread, drawing it as the hero of the empty view with a
//! crossfade on change, taking the window's colours from it, and the Settings controls.

use crate::a11y::Accessible as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::AppContext as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use super::Shell;
use super::toast::CubicBezier;
use crate::settings_widgets as w;
use crate::theme::{SPACE_SM, TITLEBAR_HEIGHT, Theme};
use crate::theme_tint::{self, rgb_of};
use crate::wallpaper::{self, Prepared};
use crate::wallpaper_fx::Effect;
use crate::wallpaper_panel::{self, Panel};

/// zeron `WALLPAPER_CROSSFADE`: an immediate attack with a short, soft landing.
pub const CROSSFADE: Duration = Duration::from_millis(180);
const CROSSFADE_CURVE: CubicBezier = CubicBezier::new(1.0 / 3.0, 1.0, 2.0 / 3.0, 1.0);
/// zeron `NEW_THREAD_BACKGROUND_VIEWPORT_RATIO` / `_MAX_HEIGHT` (crates/ui/src/shell.rs:1064-1065):
/// the hero is this share of the window's height, up to this many pixels.
const HERO_VIEWPORT_RATIO: f32 = 0.72;
const HERO_MAX_HEIGHT: f32 = 760.0;

/// Where a panel sits in the window, so its frosted copy lines up with the sharp picture.
#[derive(Clone, Copy)]
pub(super) enum Region {
    Titlebar,
    Sidebar,
    Settings,
    /// The main panel; the empty view's hero band at its top stays unfrosted.
    Main,
}

/// The empty view's panel while the picture fills the window: clear over the top `height` (the
/// picture at full strength, fading into the panel) and the panel at `alpha` below it.
fn fill_backdrop(height: f32, panel: gpui::Hsla, alpha: f32) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(height))
                .bg(gpui::linear_gradient(
                    180.0,
                    gpui::linear_color_stop(panel.opacity(0.0), 0.0),
                    gpui::linear_color_stop(panel.opacity(alpha), 1.0),
                )),
        )
        .child(
            div()
                .absolute()
                .top(px(height))
                .bottom_0()
                .left_0()
                .right_0()
                .bg(panel.opacity(alpha)),
        )
        .into_any_element()
}

/// zeron `new_thread_background_height` (shell.rs:1322).
pub(crate) fn hero_height(viewport_height: f32) -> f32 {
    (viewport_height.max(0.0) * HERO_VIEWPORT_RATIO).min(HERO_MAX_HEIGHT)
}

/// What the window should be showing: a file with an effect.
#[derive(Debug, Clone, PartialEq)]
struct Want {
    source: PathBuf,
    effect: Effect,
    /// Effects print on white paper in the light appearance, black in the dark.
    light: bool,
}

struct Shown {
    want: Want,
    prepared: Prepared,
}

struct Fade {
    /// The picture being replaced, kept under the new one until it has landed.
    from: Option<PathBuf>,
    since: Instant,
}

#[derive(Default)]
pub(super) struct State {
    /// The window's size at its last render, for placing the frosted copies.
    pub(super) viewport: (f32, f32),
    /// Height of the empty view's unfrosted hero band this frame; 0 with a tab open.
    hero_clear: f32,
    shown: Option<Shown>,
    loading: Option<Want>,
    /// A want that failed to load, so a bad file is not retried on every frame.
    failed: Option<Want>,
    fade: Option<Fade>,
    pub(super) error: Option<String>,
}

impl Shell {
    /// The wallpaper to draw: the chosen file, if it is still there.
    pub(crate) fn active_wallpaper(&self) -> Option<&str> {
        self.settings
            .wallpaper
            .as_deref()
            .filter(|p| Path::new(p).is_file())
    }

    fn wanted(&self) -> Option<Want> {
        self.active_wallpaper().map(|p| Want {
            source: PathBuf::from(p),
            effect: self.settings.wallpaper_effect,
            light: self.theme.light,
        })
    }

    /// Starts rendering whatever the settings now ask for. Called every frame; cheap when
    /// nothing changed.
    pub(super) fn sync_wallpaper(&mut self, cx: &mut Context<Self>) {
        let Some(want) = self.wanted() else {
            self.wp.failed = None;
            self.wp.error = None;
            self.wp.loading = None;
            if self.wp.shown.take().is_some() {
                self.wp.fade = None;
                // The accent is restored outside this frame: it restyles the open terminals.
                cx.spawn(async move |this, cx| {
                    this.update(cx, |s, cx| s.refresh_theme(cx)).ok();
                })
                .detach();
            }
            return;
        };
        if self.wp.shown.as_ref().is_some_and(|s| s.want == want)
            || self.wp.loading.as_ref() == Some(&want)
            || self.wp.failed.as_ref() == Some(&want)
        {
            return;
        }
        let Some(config) = crate::settings::dir() else {
            return;
        };
        self.wp.loading = Some(want.clone());
        let job = want.clone();
        let task = cx.background_spawn(async move {
            wallpaper::prepare(&job.source, job.effect, job.light, &config)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |s, cx| s.wallpaper_ready(want, result, cx))
                .ok();
        })
        .detach();
    }

    fn wallpaper_ready(
        &mut self,
        want: Want,
        result: Result<Prepared, String>,
        cx: &mut Context<Self>,
    ) {
        if self.wp.loading.as_ref() != Some(&want) {
            return;
        }
        self.wp.loading = None;
        if self.wanted().as_ref() != Some(&want) {
            // The choice moved on while this rendered; the next frame starts the new one.
            cx.notify();
            return;
        }
        match result {
            Ok(prepared) => {
                let from = self.wp.shown.as_ref().map(|s| s.prepared.image.clone());
                self.wp.fade = (!cx.reduce_motion()).then(|| Fade {
                    from,
                    since: Instant::now(),
                });
                self.wp.error = None;
                self.wp.shown = Some(Shown { want, prepared });
            }
            Err(error) => {
                tracing::warn!(error = %error, "wallpaper_failed");
                self.wp.error = Some(error);
                self.wp.failed = Some(want);
            }
        }
        self.refresh_theme(cx);
        self.restyle_tabs(cx);
        cx.notify();
    }

    /// Rebuilds the palette: tern's own, or leaning toward the wallpaper's dominant colour when
    /// asked to. Restyles open terminals and text fields when it changed.
    pub(super) fn refresh_theme(&mut self, cx: &mut Context<Self>) {
        let base = Theme::zeron(self.theme.light);
        let tint = self
            .settings
            .wallpaper_theme_colors
            .then(|| self.wp.shown.as_ref().and_then(|s| s.prepared.accent))
            .flatten();
        let next = tint.map_or(base, |color| base.tinted(color));
        if next != self.theme {
            self.theme = next;
            self.install_input_colors(cx);
            self.restyle_tabs(cx);
            cx.notify();
        }
    }

    /// The hero for the empty view, with its height. The picture is the empty view's only: with
    /// a tab or the Settings page open nothing sits behind the content.
    pub(super) fn empty_view_hero(
        &mut self,
        panel: gpui::Hsla,
        window: &mut Window,
    ) -> Option<(AnyElement, f32)> {
        self.wp.hero_clear = 0.0;
        if self.tabs.get(self.active).is_some() || self.settings_page.is_some() {
            return None;
        }
        let height = hero_height(f32::from(window.viewport_size().height));
        if let Some(alpha) = self.window_fill() {
            self.wp.hero_clear = height;
            return Some((fill_backdrop(height, panel, alpha), height));
        }
        self.wallpaper_hero(height, panel, window)
            .map(|el| (el, height))
    }

    /// The panel opacity while the picture fills the whole window; `None` in hero-only mode and
    /// without a picture. The smallest alpha at which text, muted and faint text (and the
    /// terminal's own text) keep 4.5:1 over the picture's brightest and darkest regions.
    pub(crate) fn window_fill(&self) -> Option<f32> {
        if !self.settings.wallpaper_fills_window {
            return None;
        }
        let shown = self.wp.shown.as_ref()?;
        let t = &self.theme;
        let term = t.terminal(self.settings.terminal_font_size, self.scheme_for(""));
        let words = |extra: Option<Hsla>| {
            let mut v = vec![
                (rgb_of(t.text), theme_tint::TEXT_CONTRAST),
                (rgb_of(t.muted), theme_tint::TEXT_CONTRAST),
                (rgb_of(t.faint), theme_tint::FAINT_CONTRAST),
            ];
            v.extend(extra.map(|c| (rgb_of(c), theme_tint::TEXT_CONTRAST)));
            v
        };
        let (shell_words, terminal_words) = (words(None), words(Some(term.foreground)));
        Some(wallpaper_panel::panel_alpha(
            &shown.prepared.backdrop,
            &[
                Panel {
                    surface: rgb_of(t.shell),
                    texts: &shell_words,
                },
                Panel {
                    surface: rgb_of(term.background),
                    texts: &terminal_words,
                },
            ],
        ))
    }

    /// The picture across the whole window, under everything; none in hero-only mode.
    pub(super) fn fill_layers(&mut self, window: &mut Window) -> Vec<AnyElement> {
        if self.window_fill().is_none() {
            return Vec::new();
        }
        let size = window.viewport_size();
        self.wp.viewport = (f32::from(size.width), f32::from(size.height));
        self.wallpaper_layers(window)
    }

    /// `colour` at the panel opacity while the picture fills the window, else as it is.
    pub(super) fn veil(&self, colour: Hsla) -> Hsla {
        colour.opacity(self.window_fill().unwrap_or(1.0))
    }

    /// The main panel's backdrop: the frosted picture over the window fill, else the plain
    /// panel colour.
    pub(super) fn main_backdrop(&self, panel: Hsla) -> AnyElement {
        self.frost(Region::Main)
            .unwrap_or_else(|| div().absolute().inset_0().bg(panel).into_any_element())
    }

    /// The frosted copy of the picture for one panel, as that panel's first child: the same
    /// cover-fit window-sized image offset to the panel's place, clipped to it, then the panel
    /// colour at the fill alpha over it. `None` in hero-only mode. The panel cannot paint its own
    /// fill, as that would land over this.
    pub(super) fn frost(&self, region: Region) -> Option<AnyElement> {
        use gpui::StyledImage as _;
        let alpha = self.window_fill()?;
        let shown = self.wp.shown.as_ref()?;
        let (vw, vh) = self.wp.viewport;
        let sidebar = self.sidebar_now();
        let (x, y, tint, clear) = match region {
            Region::Titlebar => (0.0, 0.0, Some(self.theme.shell), 0.0),
            Region::Sidebar => (0.0, TITLEBAR_HEIGHT, Some(self.theme.shell), 0.0),
            Region::Settings => (0.0, TITLEBAR_HEIGHT, Some(self.theme.shell), 0.0),
            // The main panel's own fill is the terminal's, or the empty view's backdrop.
            Region::Main => {
                let x = if sidebar < SPACE_SM {
                    sidebar + SPACE_SM
                } else {
                    sidebar
                };
                (x + 1.0, TITLEBAR_HEIGHT + 1.0, None, self.wp.hero_clear)
            }
        };
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .top(px(clear))
                .overflow_hidden()
                .child(
                    gpui::img(shown.prepared.blurred.clone())
                        .absolute()
                        .left(px(-x))
                        .top(px(-y - clear))
                        .w(px(vw))
                        .h(px(vh))
                        .object_fit(gpui::ObjectFit::Cover)
                        .opacity(self.settings.wallpaper_hero_opacity),
                )
                .when_some(tint, |el, c| {
                    el.child(div().absolute().inset_0().bg(c.opacity(alpha)))
                })
                .into_any_element(),
        )
    }

    /// The hero: the picture across the top of the empty view, at full strength and fading into
    /// the panel colour `panel` along its height (zeron feathers it with an alpha mask; gpui
    /// 0.3.8 has none, so a gradient to the opaque panel colour does the same). `None` without a
    /// wallpaper. The crossfade layers sit inside it.
    pub(super) fn wallpaper_hero(
        &mut self,
        height: f32,
        panel: gpui::Hsla,
        window: &mut Window,
    ) -> Option<AnyElement> {
        let layers = self.wallpaper_layers(window);
        if layers.is_empty() {
            return None;
        }
        let panel = panel.opacity(1.0);
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(height))
                .overflow_hidden()
                .children(layers)
                .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                    180.0,
                    gpui::linear_color_stop(panel.opacity(0.0), 0.0),
                    gpui::linear_color_stop(panel, 1.0),
                )))
                .into_any_element(),
        )
    }

    /// The picture layers, back to front: the one being replaced at full strength, with the new
    /// one fading in over it.
    fn wallpaper_layers(&mut self, window: &mut Window) -> Vec<AnyElement> {
        let Some(shown) = self.wp.shown.as_ref() else {
            return Vec::new();
        };
        let opacity = self.settings.wallpaper_hero_opacity;
        let current = shown.prepared.image.clone();
        let progress = self.wp.fade.as_ref().and_then(|fade| {
            let t = fade.since.elapsed().as_secs_f32() / CROSSFADE.as_secs_f32();
            (t < 1.0).then(|| (CROSSFADE_CURVE.eval(t), fade.from.clone()))
        });
        let mut layers = Vec::new();
        match progress {
            Some((eased, from)) => {
                layers.extend(from.map(|p| picture(p, opacity)));
                layers.push(picture(current, opacity * eased));
                window.request_animation_frame();
            }
            None => {
                self.wp.fade = None;
                layers.push(picture(current, opacity));
            }
        }
        layers
    }

    /// Copies the picked file into tern's folder off the UI thread, then shows it.
    fn pick_wallpaper(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose wallpaper".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let (Some(source), Some(config)) = (paths.into_iter().next(), crate::settings::dir())
            else {
                return;
            };
            let copy = cx
                .background_executor()
                .spawn(async move { wallpaper::import(&source, &config) })
                .await;
            this.update(cx, |s, cx| match copy {
                Ok(path) => s.set_wallpaper(path.to_string_lossy().into_owned(), cx),
                Err(error) => s.wp.error = Some(error),
            })
            .ok();
        })
        .detach();
    }

    /// Shows `path` (a copy in tern's folder), remembers it, and drops copies that fell out of
    /// the history.
    fn set_wallpaper(&mut self, path: String, cx: &mut Context<Self>) {
        self.wp.error = None;
        self.wp.failed = None;
        self.update_settings(
            |st| {
                wallpaper::remember(&mut st.wallpaper_history, &path);
                st.wallpaper = Some(path);
            },
            cx,
        );
        if let Some(config) = crate::settings::dir() {
            wallpaper::prune(&config, &self.settings.wallpaper_history);
        }
    }

    fn clear_wallpaper(&mut self, cx: &mut Context<Self>) {
        self.update_settings(|st| st.wallpaper = None, cx);
    }

    pub(super) fn wallpaper_card(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let name_of = |p: &str| {
            Path::new(p)
                .file_name()
                .map_or_else(|| p.to_owned(), |n| n.to_string_lossy().into_owned())
        };
        let current = match self.settings.wallpaper.as_deref() {
            Some(p) if self.active_wallpaper().is_some() => name_of(p),
            Some(_) => "File not found".to_owned(),
            None => "None".to_owned(),
        };
        let choose = w::button(&t, "wallpaper-choose", "Choose…")
            .on_click(cx.listener(|s, _, _, cx| s.pick_wallpaper(cx)));
        let remove = w::button(&t, "wallpaper-remove", "Remove")
            .on_click(cx.listener(|s, _, _, cx| s.clear_wallpaper(cx)));
        let mut card = w::card(&t).child(w::row(
            &t,
            true,
            "Image",
            Some(match &self.wp.error {
                Some(e) => e.clone().into(),
                None => format!(
                    "{current} · shown at the top of the empty view; the window takes its colours"
                )
                .into(),
            }),
            div().flex().gap(px(6.)).child(choose).child(remove),
        ));
        let recent: Vec<&String> = self
            .settings
            .wallpaper_history
            .iter()
            .filter(|p| Path::new(p).is_file())
            .collect();
        if !recent.is_empty() {
            let mut list = div()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap(px(6.))
                .max_w(px(420.));
            for (i, path) in recent.into_iter().enumerate() {
                let selected = self.settings.wallpaper.as_deref() == Some(path.as_str());
                let target = path.clone();
                list = list.child(
                    w::button(&t, ("wallpaper-recent", i), name_of(path))
                        .when(selected, |el| {
                            el.bg(t.ink(0.18)).font_weight(FontWeight::MEDIUM)
                        })
                        .on_click(
                            cx.listener(move |s, _, _, cx| s.set_wallpaper(target.clone(), cx)),
                        ),
                );
            }
            card = card.child(w::row(&t, false, "Recent", None, list));
        }
        let mut effects = div().flex().gap(px(6.));
        for (id, label, effect) in EFFECTS {
            let selected = self.settings.wallpaper_effect == effect;
            effects = effects.child(
                w::button(&t, id, label)
                    .when(selected, |el| {
                        el.bg(t.ink(0.18)).font_weight(FontWeight::MEDIUM)
                    })
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.update_settings(|st| st.wallpaper_effect = effect, cx)
                    })),
            );
        }
        let pct = (self.settings.wallpaper_hero_opacity * 100.0).round();
        let (minus, value, plus) = w::stepper(&t, "wallpaper-opacity", format!("{pct:.0}%"));
        let step = |delta: f32| {
            move |s: &mut Shell, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                s.update_settings(|st| st.wallpaper_hero_opacity += delta, cx)
            }
        };
        let on = self.settings.wallpaper_theme_colors;
        let fills = self.settings.wallpaper_fills_window;
        card.child(w::row(
            &t,
            false,
            "Effect",
            Some("Rendered once and cached".into()),
            effects,
        ))
        .child(w::row(
            &t,
            false,
            "Visibility",
            Some("How strongly the picture shows".into()),
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(minus.on_click(cx.listener(step(-0.1))))
                .child(value)
                .child(plus.on_click(cx.listener(step(0.1)))),
        ))
        .child(w::row(
            &t,
            false,
            "Wallpaper fills the window",
            Some(
                "Panels carry the text, sized to the picture; off keeps it to the empty view"
                    .into(),
            ),
            div()
                .id("toggle-wallpaper-fills")
                .switch("Wallpaper fills the window", fills)
                .cursor_pointer()
                .on_click(cx.listener(|s, _, _, cx| {
                    s.update_settings(
                        |st| st.wallpaper_fills_window = !st.wallpaper_fills_window,
                        cx,
                    );
                    s.restyle_tabs(cx);
                }))
                .child(w::toggle(&t, fills, "wallpaper-fills")),
        ))
        .child(w::row(
            &t,
            false,
            "Theme colours from wallpaper",
            Some(
                "Surfaces, accent and hover washes lean toward the image's dominant colour".into(),
            ),
            div()
                .id("toggle-wallpaper-colours")
                .switch("Theme colours from wallpaper", on)
                .cursor_pointer()
                .on_click(cx.listener(|s, _, _, cx| {
                    s.update_settings(
                        |st| st.wallpaper_theme_colors = !st.wallpaper_theme_colors,
                        cx,
                    );
                    s.refresh_theme(cx);
                }))
                .child(w::toggle(&t, on, "wallpaper-colours")),
        ))
    }
}

const EFFECTS: [(&str, &str, Effect); 5] = [
    ("wp-fx-none", "None", Effect::None),
    ("wp-fx-scanlines", "Scanlines", Effect::Scanlines),
    ("wp-fx-ascii", "ASCII", Effect::Ascii),
    ("wp-fx-halftone", "Halftone", Effect::Halftone),
    ("wp-fx-dither", "Dither", Effect::Dither),
];

fn picture(path: PathBuf, opacity: f32) -> AnyElement {
    use gpui::StyledImage as _;
    gpui::img(path)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .object_fit(gpui::ObjectFit::Cover)
        .opacity(opacity)
        .into_any_element()
}

/// The wallpaper's opacity for a crossfade `t` seconds in; exposed for the tests.
#[cfg(test)]
pub(super) fn fade_in(t: f32) -> f32 {
    CROSSFADE_CURVE.eval(t / CROSSFADE.as_secs_f32())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn crossfade_attacks_at_once_and_lands_softly() {
        assert_eq!(fade_in(0.0), 0.0);
        // zeron's curve (1/3, 1, 2/3, 1) is nearly there by the midpoint...
        assert!(fade_in(0.09) > 0.85, "{}", fade_in(0.09));
        // ...and reaches 1 exactly at the end.
        assert!((fade_in(0.18) - 1.0).abs() < 1e-3);
        assert!(fade_in(0.05) < fade_in(0.1));
    }

    #[test]
    fn hero_is_zerons_share_of_the_window_up_to_its_cap() {
        assert_eq!(hero_height(500.0), 360.0);
        assert_eq!(hero_height(2000.0), 760.0);
        assert_eq!(hero_height(-5.0), 0.0);
    }

    #[test]
    fn crossfade_matches_zerons_180_ms() {
        assert_eq!(CROSSFADE, Duration::from_millis(180));
    }
}
