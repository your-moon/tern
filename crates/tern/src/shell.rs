// Adapted from zeron crates/ui/src/lib.rs (window options) and crates/ui/src/shell.rs
// (titlebar layout) (MIT).
//! The main window: frosted shell, custom titlebar, and the main panel.

use gpui::{
    App, AppContext, Bounds, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, Styled, TitlebarOptions, Window, WindowBackgroundAppearance,
    WindowBounds, WindowControlArea, WindowOptions, div, point, px, size,
};

use crate::theme::{
    PANEL_RADIUS, SPACE_SM, TITLEBAR_HEIGHT, TITLEBAR_TOP_PAD, Theme, UI_FONT,
    titlebar_content_start,
};

pub fn open_main_window(cx: &mut App) -> anyhow::Result<()> {
    let bounds = Bounds::centered(None, size(px(1320.), px(880.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(900.), px(600.))),
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        app_owns_titlebar_drag: true,
        window_background: WindowBackgroundAppearance::Blurred,
        app_id: Some("tern".into()),
        ..Default::default()
    };
    cx.open_window(options, |_, cx| {
        cx.new(|_| Shell {
            theme: Theme::zeron_dark(),
        })
    })?;
    Ok(())
}

pub struct Shell {
    theme: Theme,
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.glass())
            .font_family(UI_FONT)
            .text_color(t.text)
            .child(titlebar(&t, window.is_fullscreen()))
            .child(
                div()
                    .flex_1()
                    .mx(px(SPACE_SM))
                    .mb(px(SPACE_SM))
                    .rounded(px(PANEL_RADIUS))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.terminal_background),
            )
    }
}

fn titlebar(t: &Theme, fullscreen: bool) -> impl IntoElement {
    div()
        .h(px(TITLEBAR_HEIGHT))
        .pt(px(TITLEBAR_TOP_PAD))
        .pl(px(titlebar_content_start(fullscreen)))
        .flex()
        .items_center()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.muted)
                .child("tern"),
        )
}
