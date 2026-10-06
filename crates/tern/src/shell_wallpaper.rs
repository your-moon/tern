// CubicBezier and the crossfade timing are zeron's (crates/ui/src/motion.rs WALLPAPER_CROSSFADE,
// MIT); the history and contrast guard follow crates/ui/src/settings/wallpaper.rs (MIT).
//! The window wallpaper: loading it off the UI thread, drawing it behind everything with a
//! crossfade on change, keeping terminal text readable over it, and the Settings controls.

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
use crate::theme::Theme;
use crate::wallpaper::{self, Prepared};
use crate::wallpaper_colors;
use crate::wallpaper_fx::Effect;

/// zeron `WALLPAPER_CROSSFADE`: an immediate attack with a short, soft landing.
pub const CROSSFADE: Duration = Duration::from_millis(180);
const CROSSFADE_CURVE: CubicBezier = CubicBezier::new(1.0 / 3.0, 1.0, 2.0 / 3.0, 1.0);
/// WCAG contrast terminal text keeps over the wallpaper.
pub const TEXT_CONTRAST: f32 = 4.5;
/// Share of a text/background pair's own contrast the wallpaper may not take away.
const KEEP: f32 = 0.85;

/// The contrast a pair must keep over the wallpaper: 85% of what it has without one, capped at
/// WCAG 4.5 (a pair that starts above 4.5/0.85 only has to stay at 4.5).
fn guard_target(base: f32) -> f32 {
    (base * KEEP).min(TEXT_CONTRAST)
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
    shown: Option<Shown>,
    loading: Option<Want>,
    /// A want that failed to load, so a bad file is not retried on every frame.
    failed: Option<Want>,
    fade: Option<Fade>,
    pub(super) error: Option<String>,
}

/// `0xRRGGBB` of a colour.
fn rgb_u32(color: Hsla) -> u32 {
    let c = gpui::Rgba::from(color);
    let ch = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
    ch(c.r) << 16 | ch(c.g) << 8 | ch(c.b)
}

fn rgb3(color: Hsla) -> [u8; 3] {
    let v = rgb_u32(color);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
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

    /// Whether a wallpaper is on screen, which makes the terminal's own fill translucent.
    pub(crate) fn has_wallpaper(&self) -> bool {
        self.wp.shown.is_some()
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
                    this.update(cx, |s, cx| s.refresh_accent(cx)).ok();
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
        self.refresh_accent(cx);
        self.restyle_tabs(cx);
        cx.notify();
    }

    /// Sets the accent from the wallpaper when asked to, else back to tern's own, and restyles
    /// open terminals when it changed.
    pub(super) fn refresh_accent(&mut self, cx: &mut Context<Self>) {
        let tinted = self
            .settings
            .wallpaper_theme_colors
            .then(|| self.wp.shown.as_ref().and_then(|s| s.prepared.accent))
            .flatten()
            .map(|color| {
                let [r, g, b] =
                    wallpaper_colors::accent_for(color, rgb3(self.theme.terminal_background));
                crate::theme::hex(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
            });
        let accent = tinted.unwrap_or_else(|| Theme::zeron(self.theme.light).accent);
        if accent != self.theme.accent {
            self.theme.accent = accent;
            self.install_input_colors(cx);
            self.restyle_tabs(cx);
            cx.notify();
        }
    }

    /// The opacity the wallpaper may have: the user's, held down where it would wash out text
    /// drawn over it — the active terminal's, the shell's, and its muted secondary text (the
    /// weakest pair). Each may lose at most `KEEP` of its own contrast, and never needs more than
    /// WCAG 4.5, measured against the brightest (or darkest) part of the image.
    pub(crate) fn wallpaper_opacity_cap(&self) -> Option<f32> {
        let shown = self.wp.shown.as_ref()?;
        let alias = self.tabs.get(self.active).map_or("", |t| t.alias.as_str());
        let term = self.terminal_theme(alias);
        let t = self.theme;
        let pairs = [
            (term.foreground, term.background),
            (t.text, t.shell),
            (t.muted, t.shell),
        ];
        Some(
            pairs
                .into_iter()
                .map(|(fg, bg)| {
                    let (fg, bg) = (rgb_u32(fg), rgb_u32(bg));
                    let target = guard_target(crate::wallpaper_fx::contrast_ratio(fg, bg));
                    wallpaper::safe_opacity(&shown.prepared.sample, fg, bg, target, 1.0)
                })
                .fold(1.0, f32::min),
        )
    }

    fn wallpaper_opacity(&self) -> f32 {
        let user = self.settings.wallpaper_opacity;
        self.wallpaper_opacity_cap()
            .map_or(user, |cap| user.min(cap))
    }

    /// The picture layers, back to front: the one being replaced at full strength, with the new
    /// one fading in over it.
    pub(super) fn wallpaper_layers(&mut self, window: &mut Window) -> Vec<AnyElement> {
        let Some(shown) = self.wp.shown.as_ref() else {
            return Vec::new();
        };
        let opacity = self.wallpaper_opacity();
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
                None => format!("{current} · PNG, JPEG or WebP, copied into tern's folder").into(),
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
        let pct = (self.settings.wallpaper_opacity * 100.0).round();
        let (minus, value, plus) = w::stepper(&t, "wallpaper-opacity", format!("{pct:.0}%"));
        let step = |delta: f32| {
            move |s: &mut Shell, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                s.update_settings(|st| st.wallpaper_opacity += delta, cx)
            }
        };
        let cap_note = self
            .wallpaper_opacity_cap()
            .filter(|cap| *cap < self.settings.wallpaper_opacity)
            .map(|cap| {
                format!(
                    "Held to {:.0}% so terminal text stays readable (WCAG {TEXT_CONTRAST})",
                    cap * 100.0
                )
                .into()
            });
        let on = self.settings.wallpaper_theme_colors;
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
            cap_note,
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
            "Theme colours from wallpaper",
            Some("Tint the accent with the image's dominant colour".into()),
            div()
                .id("toggle-wallpaper-colours")
                .switch("Theme colours from wallpaper", on)
                .cursor_pointer()
                .on_click(cx.listener(|s, _, _, cx| {
                    s.update_settings(
                        |st| st.wallpaper_theme_colors = !st.wallpaper_theme_colors,
                        cx,
                    );
                    s.refresh_accent(cx);
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
    fn rgb_round_trips() {
        assert_eq!(rgb3(crate::theme::hex(0x12_34_56)), [0x12, 0x34, 0x56]);
        assert_eq!(rgb_u32(crate::theme::hex(0xFF_FF_FF)), 0xFF_FF_FF);
    }

    #[test]
    fn terminal_text_keeps_wcag_aa_over_the_wallpaper() {
        assert_eq!(TEXT_CONTRAST, 4.5);
    }

    #[test]
    fn crossfade_matches_zerons_180_ms() {
        assert_eq!(CROSSFADE, Duration::from_millis(180));
    }
}

#[cfg(test)]
mod guard_tests {
    use super::guard_target;

    #[test]
    fn keeps_most_of_weak_contrast_and_caps_strong_at_wcag() {
        // Muted text at 4.0 must keep 3.4; it never asks for more than it had.
        assert!((guard_target(4.0) - 3.4).abs() < 1e-4);
        // Body text at 15:1 only has to stay readable at WCAG AA.
        assert!((guard_target(15.0) - 4.5).abs() < 1e-4);
        assert!(guard_target(2.0) < 2.0);
    }
}
