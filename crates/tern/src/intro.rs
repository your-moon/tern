//! The character reveal at the moon, adapted from Zeron's desktop intro (MIT).
use gpui::{Bounds, IntoElement, Styled, canvas, div, fill, point, px, rgb, size};

use super::{
    AnyElement, Context, ParentElement, Shell, StatefulInteractiveElement, Window, connections_ui,
};
use gpui::{InteractiveElement as _, prelude::FluentBuilder};

fn reveal(time: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let radius = if time < 1.8 {
                0.5 * (1. - (1. - time / 1.8).powi(3))
            } else {
                let phase = (time - 1.8) / 2.8;
                0.5 + 0.8 * (1. - (phase * std::f32::consts::PI).cos()) * 0.5
            } * (width * 0.5).hypot(height * 0.62);
            let mut paint = |x, y, w, h, color| {
                window.paint_quad(fill(
                    Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w), px(h))),
                    rgb(color),
                ));
            };
            let cell = 10.;
            for row in 0..(height / cell).ceil() as i32 {
                let y = row as f32 * cell;
                let dy = y - height * 0.62;
                let span = (radius * radius - dy * dy).max(0.).sqrt();
                let start = (width * 0.5 - span).max(0.);
                let end = (width * 0.5 + span).min(width);
                paint(0., y, start, cell, 0x101724);
                paint(end, y, width - end, cell, 0x101724);
                for edge in [start, end] {
                    if span > cell && edge > cell && edge < width - cell {
                        let bits = (row * 13 + (time * 14.) as i32) % 5;
                        paint(edge - 2., y + 3., 4., 1., 0xa9c5d0);
                        if bits < 3 {
                            paint(edge, y + 1., 1., 5., 0xa9c5d0);
                        }
                    }
                }
            }
        },
    )
    .absolute()
    .size_full()
}

impl Shell {
    pub(super) fn welcome(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use gpui::StyledImage as _;
        let accent = self.theme.accent;
        let elapsed = self.launched.elapsed().as_secs_f32();
        let time = if cx.reduce_motion() { 11. } else { elapsed };
        if time < 11. {
            window.request_animation_frame();
        }
        // Zeron intro.luau uses inOutSine for both dawn transitions.
        let fade = |delay: f32, duration: f32| {
            let phase = ((time - delay) / duration).clamp(0., 1.);
            (1. - (phase * std::f32::consts::PI).cos()) * 0.5
        };
        let plate = |asset: &'static str, opacity| {
            gpui::img(asset)
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .opacity(opacity)
        };
        let cloud = |asset, from: f32, progress| {
            plate(asset, progress).left(gpui::relative(from * (1. - progress)))
        };
        let banks = 1. - (1. - ((time - 2.4) / 4.2).clamp(0., 1.)).powi(3);
        div()
            .tab_group()
            .relative()
            .overflow_hidden()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(gpui::rgb(0x101724))
            .child(plate("intro/plate-1-night.jpg", fade(0., 1.)))
            .child(plate("intro/plate-2-bluehour.jpg", fade(1.7, 1.7)))
            .child(plate("intro/plate-3-sunrise.jpg", fade(3.5, 2.)))
            .child(cloud("intro/clouds-mid.png", 0.05, fade(3., 8.)))
            .child(cloud("intro/clouds-left.png", -0.16, banks))
            .child(cloud("intro/clouds-right.png", 0.16, banks))
            .when(time < 4.6, |scene| scene.child(reveal(time)))
            .child(div().absolute().size_full().bg(gpui::rgba(0x10172488)))
            .when(!self.intro_entered, |scene| {
                scene.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(16.))
                        .text_color(gpui::white())
                        .child(div().text_size(px(42.)).child("Your hosts, anywhere."))
                        .child(
                            connections_ui::button(
                                "welcome-enter",
                                "Get started",
                                true,
                                &self.theme,
                            )
                            .tab_index(0)
                            .focus_visible(move |el| el.border_1().border_color(accent))
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.intro_entered = true;
                                cx.notify();
                            })),
                        ),
                )
            })
            .when(self.intro_entered, |scene| {
                scene.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
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
