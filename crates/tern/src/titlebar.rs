// Adapted from zeron crates/ui/src/shell.rs (traffic-light-aware titlebar layout) (MIT).
//! The custom titlebar: app name, tabs, and an empty strip that drags the window.

use gpui::{
    FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement, Styled,
    WindowControlArea, div, px,
};

use crate::theme::{SPACE_SM, TITLEBAR_HEIGHT, TITLEBAR_TOP_PAD, Theme, titlebar_content_start};

pub fn render(t: &Theme, fullscreen: bool, tabs: impl IntoElement) -> impl IntoElement {
    div()
        .h(px(TITLEBAR_HEIGHT))
        .flex_none()
        .pt(px(TITLEBAR_TOP_PAD))
        .pl(px(titlebar_content_start(fullscreen)))
        .pr(px(SPACE_SM))
        .flex()
        .items_center()
        .gap(px(SPACE_SM))
        .child(
            div()
                .flex_none()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.muted)
                .child("tern"),
        )
        .child(tabs)
        .child(
            div()
                // The drag strip keeps a grab area even when the tabs fill the bar.
                .flex_1()
                .min_w(px(48.))
                .h_full()
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
        )
}
