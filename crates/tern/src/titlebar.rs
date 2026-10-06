// Adapted from zeron crates/ui/src/shell.rs (traffic-light-aware titlebar layout) (MIT).
//! The custom titlebar: app name, tabs, and an empty strip that drags the window.

use crate::a11y::Accessible as _;
use crate::hover::HoverFade as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    InteractiveElement, IntoElement, MouseButton, ParentElement, StatefulInteractiveElement,
    Styled, WindowControlArea, div, px,
};

use crate::theme::{SPACE_SM, TITLEBAR_HEIGHT, TITLEBAR_TOP_PAD, Theme, titlebar_content_start};

/// zeron's titlebar cluster: the sidebar toggle and + (find a host) at x = 88, 24 pt buttons,
/// 16 pt icons, then the tabs.
pub fn render(
    t: &Theme,
    fullscreen: bool,
    sidebar_open: bool,
    tabs: impl IntoElement,
    tint: Option<gpui::Hsla>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .h(px(TITLEBAR_HEIGHT))
        .flex_none()
        .when_some(tint, |el, c| el.bg(c))
        .pt(px(TITLEBAR_TOP_PAD))
        .pl(px(titlebar_content_start(fullscreen)))
        .pr(px(SPACE_SM))
        .flex()
        .items_center()
        .gap(px(SPACE_SM))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .child(
                    control("toggle-sidebar", t)
                        .icon_button("Toggle sidebar", t)
                        .on_click(cx.listener(|s, _, _, cx| s.toggle_sidebar(cx)))
                        .child(crate::icons::sidebar_glyph(sidebar_open, 16., t.muted)),
                )
                .child(
                    control("find-host", t)
                        .icon_button("New tab: find a host", t)
                        .on_click(cx.listener(|s, _, w, cx| s.toggle_picker(w, cx)))
                        .child(
                            crate::icons::icon(crate::icons::PLUS)
                                .size(px(16.))
                                .text_color(t.muted),
                        ),
                ),
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

fn control(id: &'static str, t: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover_fade(
            format!("titlebar-{id}"),
            gpui::transparent_black(),
            t.ink(0.11),
        )
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
