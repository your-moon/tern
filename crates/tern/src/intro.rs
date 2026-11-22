// Adapted from zeron apps/zgx/ui/intro.luau (MIT).
//! The first-host scene follows the moon reveal, staggered words and setup handoff.
#[path = "intro_marks.rs"]
mod marks;

use gpui::{Bounds, IntoElement, Styled, canvas, div, fill, point, px, rgb, size};

use super::{
    AnyElement, Context, ParentElement, Shell, StatefulInteractiveElement, Window, connections_ui,
};
use gpui::{InteractiveElement as _, prelude::FluentBuilder};

// The shader's 5×7 font, packed five bits per row, and its 32-bit scrambling hash.
// Ported from zgx-gpu/src/image_effects.wgsl at e3f35a54 (MIT).
const GLYPHS: [(u32, u32); 11] = [
    (0, 0),
    (402653184, 12),
    (1015808, 0),
    (139432064, 0),
    (340083712, 17),
    (581052977, 17),
    (35078671, 30),
    (588727854, 14),
    (597347886, 14),
    (368409930, 10),
    (561700398, 15),
];

fn cell_hash(x: u32, y: u32) -> f32 {
    let mut h = x
        .wrapping_mul(1664525)
        .wrapping_add(y.wrapping_mul(1013904223))
        .wrapping_add(0x9e3779b9);
    h ^= h >> 16;
    h = h.wrapping_mul(2246822519);
    h ^= h >> 13;
    h = h.wrapping_mul(3266489917);
    ((h ^ (h >> 16)) & 16777215) as f32 / 16777216.
}

fn glyph_atlas() -> &'static [(gpui::SharedString, String); 11] {
    static ATLAS: std::sync::OnceLock<[(gpui::SharedString, String); 11]> =
        std::sync::OnceLock::new();
    ATLAS.get_or_init(|| std::array::from_fn(|index| {
        let (top, bottom) = GLYPHS[index];
        let mut path = String::new();
        for y in 0..7 {
            let bits = if y == 6 { bottom } else { top >> (5*y) };
            for x in 0..5 {
                if bits & (1 << (4-x)) != 0 { path.push_str(&format!("M{x} {y}h1v1h-1z")); }
            }
        }
        (format!("intro-glyph-{index}").into(), format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="6" height="8" viewBox="0 0 6 8"><path d="{path}"/></svg>"#))
    }))
}

fn smooth(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

const LAYERS: [&str; 6] = [
    "intro/plate-1-night.jpg",
    "intro/plate-2-bluehour.jpg",
    "intro/plate-3-sunrise.jpg",
    "intro/clouds-mid.png",
    "intro/clouds-left.png",
    "intro/clouds-right.png",
];

fn reveal(time: f32, alphas: [f32; 6], offsets: [f32; 6]) -> impl IntoElement {
    canvas(
        |_, window, cx| {
            LAYERS.map(|path| {
                window
                    .use_asset::<gpui::ImgResourceLoader>(
                        &gpui::Resource::Embedded(path.into()),
                        cx,
                    )
                    .and_then(Result::ok)
            })
        },
        move |bounds, plates, window, cx| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            if width < 1. || height < 1. {
                return;
            }
            let phase = ((time - 1.8) / 2.8).clamp(0., 1.);
            let cubic = if phase < 0.5 {
                4. * phase.powi(3)
            } else {
                1. - (-2. * phase + 2.).powi(3) / 2.
            };
            let front = if time < 1.8 {
                0.5 * (1. - (1. - time / 1.8).powi(3))
            } else {
                0.5 + 0.8 * cubic
            };
            let horizon = 0.5 + 0.12 * (1. - cubic);
            let ragged = 0.14 + 0.14 * drift_phase(time, 4.6, 6.5);
            let atlas = glyph_atlas();
            let images = plates.each_ref().map(|img| {
                img.as_ref().and_then(|img| {
                    let sz = img.size(0);
                    let w = sz.width.0 as usize;
                    let h = sz.height.0 as usize;
                    let scale = (1.12 * width / w as f32).max(height / h as f32);
                    Some((img.as_bytes(0)?, w, h, scale))
                })
            });
            for row in 0..(height / 16.).ceil() as u32 {
                let y = row as f32 * 16.;
                let v = ((y + 8.) / height - horizon) * 2.;
                for col in 0..(width / 12.).ceil() as u32 {
                    let x = col as f32 * 12.;
                    let u = ((x + 6.) / width - 0.5) * 2.;
                    let depth = front - (u.powi(4) + v.powi(4)).sqrt().sqrt();
                    let shown = smooth(0.36 * 0.35, 0.36 * 1.35, depth);
                    let cell =
                        Bounds::new(bounds.origin + point(px(x), px(y)), size(px(12.), px(16.)));
                    if shown < 1. {
                        window.paint_quad(fill(
                            cell,
                            gpui::Hsla::from(rgb(0x101724)).opacity(1. - shown),
                        ));
                    }
                    let into = (depth + (cell_hash(col, row) - 0.5) * ragged) / 0.36;
                    if into <= 0. || cell_hash(col + 77, row + 13) >= into.clamp(0., 1.).powf(0.7) {
                        continue;
                    }
                    let mut color = [0.; 3];
                    for ((source, alpha), offset) in images.iter().zip(alphas).zip(offsets) {
                        if let Some((bytes, w, h, scale)) = source {
                            let sx = (((x + 6. - width * (0.5 + offset)) / scale + *w as f32 * 0.5)
                                as usize)
                                .min(w - 1);
                            let sy = (((y + 8. - height * 0.5) / scale + *h as f32 * 0.5) as usize)
                                .min(h - 1);
                            let pixel = &bytes[(sy * w + sx) * 4..][..4];
                            for (dst, &src) in color.iter_mut().zip(pixel[..3].iter().rev()) {
                                *dst +=
                                    (src as f32 / 255. - *dst) * alpha * f32::from(pixel[3]) / 255.;
                            }
                        }
                    }
                    let [r, g, b] = color;
                    let index = if into < 1. {
                        let step = (into * 12.) as u32;
                        1 + (cell_hash(col + step * 7 + 1, row + step) * 9.999) as usize
                    } else {
                        (((0.299 * r + 0.587 * g + 0.114 * b - 0.3) / 0.65)
                            .clamp(0., 1.)
                            .powf(1.6)
                            * 10.999) as usize
                    };
                    if index == 0 {
                        continue;
                    }
                    let peak = r.max(g).max(b).max(0.25);
                    let ink = gpui::Rgba {
                        r: r / peak,
                        g: g / peak,
                        b: b / peak,
                        a: smooth(0., 0.6, into) * (1. - 0.35 * shown),
                    };
                    let (key, svg) = &atlas[index];
                    let _ = window.paint_svg(
                        cell,
                        key.clone(),
                        Some(svg.as_bytes()),
                        Default::default(),
                        ink.into(),
                        cx,
                    );
                }
            }
        },
    )
    .absolute()
    .size_full()
}

fn drift_phase(time: f32, start: f32, period: f32) -> f32 {
    (1. - (((time - start).max(0.) / period) * std::f32::consts::PI).cos()) * 0.5
}

impl Shell {
    fn enter_intro(&mut self, cx: &mut Context<Self>) {
        if cx.reduce_motion() {
            self.intro_entered = true;
        } else {
            self.intro_handoff
                .get_or_insert_with(std::time::Instant::now);
        }
        cx.notify();
    }

    pub(super) fn intro_escape(
        &mut self,
        key: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.showing_intro()
            || self.picker.is_some()
            || self.form.is_some()
            || self.context_menu.is_some()
            || key.keystroke.key != "escape"
            || key.keystroke.modifiers != gpui::Modifiers::default()
        {
            return false;
        }
        self.enter_intro(cx);
        true
    }

    pub(super) fn advance_intro(&mut self, cx: &Context<Self>) {
        if let Some(start) = self.intro_handoff
            && (cx.reduce_motion() || start.elapsed().as_secs_f32() >= 1.7 || self.intro_entered)
        {
            self.intro_entered = true;
            self.intro_handoff = None;
        }
    }

    pub(super) fn welcome(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use gpui::StyledImage as _;
        let accent = self.theme.accent;
        let elapsed = self.launched.elapsed().as_secs_f32();
        let time = if cx.reduce_motion() || self.intro_entered {
            11.
        } else {
            self.intro_handoff.map_or(elapsed, |start| {
                start.saturating_duration_since(self.launched).as_secs_f32()
            })
        };
        let handoff = self
            .intro_handoff
            .map(|start| start.elapsed().as_secs_f32());
        if (!self.intro_entered && !cx.reduce_motion()) || handoff.is_some() {
            window.request_animation_frame();
        }
        // Zeron intro.luau uses inOutSine for both dawn transitions.
        let fade = |delay: f32, duration: f32| {
            let phase = ((time - delay) / duration).clamp(0., 1.);
            (1. - (phase * std::f32::consts::PI).cos()) * 0.5
        };
        let banks = 1. - (1. - ((time - 2.4) / 4.2).clamp(0., 1.)).powi(3);
        let out_sine = |start: f32, duration: f32| {
            (((time - start) / duration).clamp(0., 1.) * std::f32::consts::FRAC_PI_2).sin()
        };
        let camera = 0.03 - 0.06 * out_sine(0., 11.) + 0.022 * drift_phase(time, 11., 15.);
        let offsets = [
            camera,
            camera,
            camera,
            0.05 - 0.07 * out_sine(3., 8.) + 0.026 * drift_phase(time, 11., 12.5),
            -0.16 * (1. - banks) + 0.016 * drift_phase(time, 6.6, 9.),
            0.16 * (1. - banks) - 0.013 * drift_phase(time, 6.6, 11.),
        ];
        let alphas = [
            1. - (1. - ((time - 0.12) / 0.3).clamp(0., 1.)).powi(2),
            fade(1.7, 1.7),
            fade(3.5, 2.),
            out_sine(3., 8.),
            banks,
            banks,
        ];
        let departure = handoff.unwrap_or(0.);
        let title_opacity = 1. - (departure / 0.18).min(1.).powi(2);
        let setup_progress = if self.intro_entered {
            1.
        } else {
            (departure / 1.7).min(1.)
        };
        let ease_out = |start: f32, duration: f32| {
            let t = ((time - start) / duration).clamp(0., 1.);
            if t == 1. {
                1.
            } else {
                1. - 2_f32.powf(-10. * t)
            }
        };
        let words = ["Your", "hosts,", "anywhere."]
            .into_iter()
            .enumerate()
            .map(|(i, word)| {
                let opacity = ease_out(6.3 + i as f32 * 0.14, 1.1);
                div()
                    .relative()
                    .top(px(18. * (1. - opacity)))
                    .opacity(opacity)
                    .when(i == 2, |el| el.italic())
                    .child(word)
            });
        let button_progress = ease_out(7.4, 0.9);
        div()
            .tab_group()
            .relative()
            .overflow_hidden()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(if self.intro_entered {
                self.theme.shell
            } else {
                gpui::rgb(0x101724).into()
            })
            .when(!self.intro_entered, |scene| {
                scene
                    .children(LAYERS.into_iter().enumerate().map(|(i, asset)| {
                        gpui::img(asset)
                            .absolute()
                            .top_0()
                            .left(gpui::relative(-0.06 + offsets[i]))
                            .h_full()
                            .w(gpui::relative(1.12))
                            .object_fit(gpui::ObjectFit::Cover)
                            .opacity(alphas[i])
                    }))
                    .child(reveal(time, alphas, offsets))
            })
            .when(!self.intro_entered && handoff.is_none(), |scene| {
                scene.child(marks::scene(time))
            })
            // The reference handoff fades the live scene when a GPU snapshot is unavailable.
            .when(handoff.is_some(), |scene| {
                scene.child(
                    div()
                        .absolute()
                        .size_full()
                        .bg(self.theme.shell.opacity((departure / 0.9).min(1.).powi(2))),
                )
            })
            .when(!self.intro_entered, |scene| {
                scene
                    .child(
                        div()
                            .absolute()
                            .top(gpui::relative(0.35))
                            .left_0()
                            .right_0()
                            .flex()
                            .justify_center()
                            .gap(px(16.))
                            .font_family("Instrument Serif")
                            .text_size(px(80.))
                            .text_color(gpui::white())
                            .opacity(title_opacity)
                            .children(words),
                    )
                    .when(button_progress > 0., |scene| {
                        scene.child(
                            div()
                                .absolute()
                                .top(gpui::relative(0.48))
                                .left_0()
                                .right_0()
                                .flex()
                                .justify_center()
                                .opacity(button_progress * title_opacity)
                                .child(
                                    div()
                                        .id("welcome-enter")
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .h(px(40.))
                                        .px(px(24.))
                                        .gap(px(8.))
                                        .rounded(px(20.))
                                        .border_1()
                                        .border_color(gpui::white())
                                        .bg(gpui::white())
                                        .text_size(px(15.))
                                        .text_color(rgb(0x0a0a0a))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(0xededed)))
                                        .active(|s| s.bg(rgb(0xe2e2e2)))
                                        .child("Get started")
                                        .child("→")
                                        .relative()
                                        .top(px(10. * (1. - button_progress)))
                                        .tab_index(0)
                                        .focus_visible(move |el| el.border_1().border_color(accent))
                                        .on_click(cx.listener(|s, _, _, cx| s.enter_intro(cx))),
                                ),
                        )
                    })
            })
            .when(self.intro_entered || handoff.is_some(), |scene| {
                scene.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .relative()
                        .left(px(if self.intro_entered {
                            0.
                        } else {
                            self.sidebar_target() * 0.5 * setup_progress
                        }))
                        .opacity(1. - (1. - setup_progress).powi(2))
                        .p(px(28.))
                        .rounded(px(14.))
                        .bg(self.theme.shell)
                        .text_color(self.theme.text)
                        .child(div().text_size(px(34.)).child("Connect your first host"))
                        .child(
                            div()
                                .text_sm()
                                .child("Start with an SSH address or import your existing hosts."),
                        )
                        .child(
                            div()
                                .mt(px(12.))
                                .flex()
                                .gap(px(8.))
                                .child(
                                    connections_ui::button(
                                        "welcome-new",
                                        "New connection",
                                        true,
                                        &self.theme,
                                    )
                                    .tab_index(0)
                                    .focus_visible(move |el| el.border_1().border_color(accent))
                                    .on_click(
                                        cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx)),
                                    ),
                                )
                                .child(
                                    connections_ui::button(
                                        "welcome-import",
                                        "Import SSH config",
                                        false,
                                        &self.theme,
                                    )
                                    .tab_index(1)
                                    .focus_visible(move |el| el.border_1().border_color(accent))
                                    .on_click(cx.listener(|s, _, w, cx| s.open_import(w, cx))),
                                ),
                        ),
                )
            })
            .into_any_element()
    }
}
