// Adapted from zeron crates/ui/src/lib.rs (window options) and crates/ui/src/shell.rs
// (titlebar layout) (MIT).
//! The main window: frosted shell, custom titlebar, host sidebar and the main panel.

use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, SharedString, Styled, Subscription,
    TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowControlArea,
    WindowHandle, WindowOptions, div, point, px, size,
};
use tern_ssh::{ConnectSpec, HostEntry};

use crate::session::Session;
use crate::sidebar;
use crate::theme::{
    PANEL_RADIUS, SPACE_SM, TITLEBAR_HEIGHT, TITLEBAR_TOP_PAD, Theme, UI_FONT,
    titlebar_content_start,
};

pub fn open_main_window(cx: &mut App) -> anyhow::Result<WindowHandle<Shell>> {
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
    let window = cx.open_window(options, |_, cx| {
        cx.new(|_| Shell {
            theme: Theme::zeron_dark(),
            hosts: tern_ssh::load_ssh_config_hosts(),
            active: None,
            error: None,
        })
    })?;
    Ok(window)
}

pub struct Shell {
    theme: Theme,
    hosts: Vec<HostEntry>,
    active: Option<Active>,
    error: Option<String>,
}

struct Active {
    alias: String,
    session: Entity<Session>,
    _repaint: Subscription,
}

impl Shell {
    pub fn connect_host(&mut self, host: HostEntry, window: &mut Window, cx: &mut Context<Self>) {
        match ConnectSpec::from_host_entry(&host) {
            Ok(spec) => self.open(spec, host.alias, window, cx),
            Err(e) => self.fail(e.to_string(), cx),
        }
    }

    /// `tern <target>`: a `~/.ssh/config` alias or `user@host:port`.
    pub fn connect_target(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        match ConnectSpec::parse(target) {
            Ok(spec) => self.open(spec, target.to_string(), window, cx),
            Err(e) => self.fail(e.to_string(), cx),
        }
    }

    /// Replaces the current session; dropping the old one closes its connection.
    fn open(
        &mut self,
        spec: ConnectSpec,
        alias: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = Session::open(spec, &self.theme, window, cx);
        let repaint = cx.observe(&session, |_, _, cx| cx.notify());
        self.active = Some(Active {
            alias,
            session,
            _repaint: repaint,
        });
        self.error = None;
        cx.notify();
    }

    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        tracing::warn!(error = %error, "connect_target_invalid");
        self.error = Some(error);
        cx.notify();
    }

    fn panel_content(&self, cx: &App) -> AnyElement {
        if let Some(active) = &self.active {
            return active.session.read(cx).view.clone().into_any_element();
        }
        let message = self
            .error
            .clone()
            .unwrap_or_else(|| "Select a host to connect".into());
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(self.theme.muted)
            .child(SharedString::from(message))
            .into_any_element()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let active = self
            .active
            .as_ref()
            .map(|a| (a.alias.clone(), a.session.read(cx).status.clone()));
        let label = active.as_ref().map(|(alias, _)| alias.clone());
        let sidebar = sidebar::render(
            &self.hosts,
            active
                .as_ref()
                .map(|(alias, status)| (alias.as_str(), status)),
            &t,
            cx,
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.glass())
            .font_family(UI_FONT)
            .text_color(t.text)
            .child(titlebar(&t, window.is_fullscreen(), label))
            .child(
                div().flex_1().min_h_0().flex().child(sidebar).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .mr(px(SPACE_SM))
                        .mb(px(SPACE_SM))
                        .rounded(px(PANEL_RADIUS))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.terminal_background)
                        .overflow_hidden()
                        .child(self.panel_content(cx)),
                ),
            )
    }
}

fn titlebar(t: &Theme, fullscreen: bool, session: Option<String>) -> impl IntoElement {
    div()
        .h(px(TITLEBAR_HEIGHT))
        .pt(px(TITLEBAR_TOP_PAD))
        .pl(px(titlebar_content_start(fullscreen)))
        .flex()
        .items_center()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
        .gap(px(SPACE_SM))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.muted)
                .child("tern"),
        )
        .children(session.map(|s| {
            div()
                .text_sm()
                .text_color(t.faint)
                .child(SharedString::from(s))
        }))
}
