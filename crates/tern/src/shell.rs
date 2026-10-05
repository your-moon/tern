// Adapted from zeron crates/ui/src/lib.rs (window options) (MIT).
//! The main window: frosted shell, titlebar with tabs, host sidebar and the main panel.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, MouseUpEvent, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Subscription, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowOptions, actions, div, point, px,
    size,
};
use tern_ssh::{ConnectSpec, HostEntry};

use crate::pane::{self, DragGhost, SidebarResize, WidthTween};
use crate::picker::{self, Picker, ToggleHostPicker};
use crate::session::{Session, Status};
use crate::settings::{self, SIDEBAR_DEFAULT, Settings};
use crate::tabs::{self, ActivateTab, CloseTab, NextTab, PrevTab, TabInfo};
use crate::theme::{PANEL_RADIUS, SPACE_SM, Theme, UI_FONT};
use crate::{sidebar, titlebar};

actions!(
    tern,
    [
        ToggleSidebar,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize
    ]
);

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
        cx.new(|cx| Shell {
            focus: cx.focus_handle(),
            settings: settings::dir()
                .map(|d| Settings::load(&d))
                .unwrap_or_default(),
            theme: Theme::zeron_dark(),
            hosts: tern_ssh::load_ssh_config_hosts(),
            tabs: Vec::new(),
            active: 0,
            error: None,
            picker: None,
            sidebar_tween: None,
        })
    })?;
    // With no tab open nothing else holds focus, and gpui only dispatches key bindings along
    // the focused element's path, so the shell itself must be focused for ⌘K to work.
    window.update(cx, |shell, window, cx| window.focus(&shell.focus, cx))?;
    Ok(window)
}

pub struct Shell {
    focus: FocusHandle,
    settings: Settings,
    theme: Theme,
    hosts: Vec<HostEntry>,
    tabs: Vec<Tab>,
    active: usize,
    error: Option<String>,
    picker: Option<Picker>,
    sidebar_tween: Option<WidthTween>,
}

struct Tab {
    alias: String,
    session: Entity<Session>,
    _repaint: Subscription,
}

impl Shell {
    /// Switches to the host's tab if it has one, reconnecting it when closed; otherwise opens
    /// a new tab.
    pub fn connect_host(&mut self, host: HostEntry, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|tab| tab.alias == host.alias) {
            let session = self.tabs[ix].session.clone();
            session.update(cx, |s, cx| {
                if s.status == Status::Closed {
                    s.reconnect(cx);
                }
            });
            return self.activate_tab(ix, window, cx);
        }
        match ConnectSpec::from_host_entry(&host) {
            Ok(spec) => self.open_tab(spec, host.alias, window, cx),
            Err(e) => self.fail(e.to_string(), cx),
        }
    }

    /// `tern <target>`: a `~/.ssh/config` alias or `user@host:port`.
    pub fn connect_target(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        match ConnectSpec::parse(target) {
            Ok(spec) => self.open_tab(spec, target.to_string(), window, cx),
            Err(e) => self.fail(e.to_string(), cx),
        }
    }

    fn open_tab(
        &mut self,
        spec: ConnectSpec,
        alias: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme = self.terminal_theme();
        let session = Session::open(spec, theme, window, cx);
        let repaint = cx.observe(&session, |_, _, cx| cx.notify());
        self.tabs.push(Tab {
            alias,
            session,
            _repaint: repaint,
        });
        self.error = None;
        self.activate_tab(self.tabs.len() - 1, window, cx);
    }

    pub fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        self.active = ix;
        let focus = tab.session.read(cx).view.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Closing a tab drops its session, which ends the connection.
    pub fn close_tab_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.tabs.remove(ix);
        if self.tabs.is_empty() {
            self.active = 0;
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let next = if self.active > ix || self.active == self.tabs.len() {
            self.active - 1
        } else {
            self.active
        };
        self.activate_tab(next, window, cx);
    }

    pub fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        tracing::debug!(open = self.picker.is_some(), "picker_toggle");
        if self.picker.is_some() {
            return self.close_picker(window, cx);
        }
        let picker = Picker::new(cx);
        window.focus(picker.focus(), cx);
        self.picker = Some(picker);
        cx.notify();
    }

    /// Closing hands focus back to the active terminal, so typing resumes where it was.
    pub fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.take().is_none() {
            return;
        }
        let focus = match self.tabs.get(self.active) {
            Some(tab) => tab.session.read(cx).view.focus_handle(cx),
            None => self.focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Applies a settings change and writes it out; a failed write is logged, not fatal.
    pub fn update_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        change(&mut self.settings);
        self.settings = self.settings.clone().clamped();
        if let Some(dir) = settings::dir()
            && let Err(e) = self.settings.save(&dir)
        {
            tracing::warn!(error = %e, "settings_save_failed");
        }
        cx.notify();
    }

    fn sidebar_target(&self) -> f32 {
        if self.settings.sidebar_collapsed {
            0.0
        } else {
            self.settings.sidebar_width
        }
    }

    /// The sidebar's width this frame: mid-tween while collapsing or expanding.
    fn sidebar_now(&self) -> f32 {
        self.sidebar_tween
            .and_then(|tween| tween.sample(std::time::Instant::now()))
            .unwrap_or_else(|| self.sidebar_target())
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        let from = self.sidebar_now();
        self.update_settings(|s| s.sidebar_collapsed = !s.sidebar_collapsed, cx);
        self.sidebar_tween = Some(WidthTween::new(from, self.sidebar_target()));
    }

    /// Live drag: follows the pointer without writing the file on every move.
    fn on_sidebar_drag(&mut self, x: f32, cx: &mut Context<Self>) {
        self.settings.sidebar_width = pane::dragged_width(x);
        self.settings.sidebar_collapsed = false;
        self.sidebar_tween = None;
        cx.notify();
    }

    fn resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("sidebar-resize")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(self.sidebar_now() - pane::HANDLE_HALF_WIDTH))
            .w(px(pane::HANDLE_HALF_WIDTH * 2.0))
            .occlude()
            .cursor_col_resize()
            .on_drag(SidebarResize, |_, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| DragGhost)
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, event: &MouseUpEvent, _, cx| {
                    let reset = event.click_count == 2;
                    shell.update_settings(
                        |s| {
                            if reset {
                                s.sidebar_width = SIDEBAR_DEFAULT;
                            }
                        },
                        cx,
                    );
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|shell, _, _, cx| shell.update_settings(|_| {}, cx)),
            )
    }

    fn terminal_theme(&self) -> tern_term::TerminalTheme {
        self.theme.terminal(self.settings.terminal_font_size)
    }

    /// Font size applies to every tab at once; each terminal re-measures its cells and the
    /// remote side is told the new grid size.
    fn change_font(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        self.update_settings(change, cx);
        let theme = self.terminal_theme();
        for tab in &self.tabs {
            let view = tab.session.read(cx).view.clone();
            view.update(cx, |v, cx| v.set_theme(theme.clone(), cx));
        }
    }

    pub fn hosts(&self) -> &[HostEntry] {
        &self.hosts
    }

    pub fn picker_query(&self) -> String {
        self.picker
            .as_ref()
            .map(|p| p.query().to_string())
            .unwrap_or_default()
    }

    pub fn picker_mut(&mut self) -> Option<&mut Picker> {
        self.picker.as_mut()
    }

    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        tracing::warn!(error = %error, "connect_target_invalid");
        self.error = Some(error);
        cx.notify();
    }

    fn tab_infos(&self, cx: &App) -> Vec<TabInfo> {
        self.tabs
            .iter()
            .map(|tab| TabInfo {
                alias: tab.alias.clone(),
                status: tab.session.read(cx).status.clone(),
            })
            .collect()
    }

    fn panel_content(&self, cx: &App) -> AnyElement {
        if let Some(tab) = self.tabs.get(self.active) {
            return tab.session.read(cx).view.clone().into_any_element();
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
        let infos = self.tab_infos(cx);
        let active_alias = infos.get(self.active).map(|i| i.alias.clone());
        let strip = tabs::strip(&infos, self.active, &t, cx);
        let sidebar = sidebar::render(
            &self.hosts,
            &infos,
            active_alias.as_deref(),
            self.settings.sidebar_width,
            &t,
            cx,
        );
        let sidebar_now = self.sidebar_now();
        if self.sidebar_tween.is_some() {
            if sidebar_now == self.sidebar_target() {
                self.sidebar_tween = None;
            } else {
                window.request_animation_frame();
            }
        }
        let collapsed = self.settings.sidebar_collapsed;
        let handle = (!collapsed && self.sidebar_tween.is_none()).then(|| self.resize_handle(cx));
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(t.glass())
            .font_family(UI_FONT)
            .text_color(t.text)
            .on_action(cx.listener(|s, _: &CloseTab, w, cx| s.close_tab_at(s.active, w, cx)))
            .on_action(cx.listener(|s, _: &NextTab, w, cx| {
                s.activate_tab(tabs::step(s.active, s.tabs.len(), 1), w, cx)
            }))
            .on_action(cx.listener(|s, _: &PrevTab, w, cx| {
                s.activate_tab(tabs::step(s.active, s.tabs.len(), -1), w, cx)
            }))
            .on_action(cx.listener(|s, a: &ActivateTab, w, cx| s.activate_tab(a.0, w, cx)))
            .on_action(cx.listener(|s, _: &ToggleHostPicker, w, cx| s.toggle_picker(w, cx)))
            .on_action(cx.listener(|s, _: &ToggleSidebar, _, cx| s.toggle_sidebar(cx)))
            .on_action(cx.listener(|s, _: &IncreaseFontSize, _, cx| {
                s.change_font(|st| st.step_font(1.0), cx)
            }))
            .on_action(cx.listener(|s, _: &DecreaseFontSize, _, cx| {
                s.change_font(|st| st.step_font(-1.0), cx)
            }))
            .on_action(cx.listener(|s, _: &ResetFontSize, _, cx| {
                s.change_font(|st| st.terminal_font_size = settings::FONT_DEFAULT, cx)
            }))
            .on_drag_move(
                cx.listener(|s, e: &gpui::DragMoveEvent<SidebarResize>, _, cx| {
                    s.on_sidebar_drag(f32::from(e.event.position.x), cx)
                }),
            )
            .child(titlebar::render(&t, window.is_fullscreen(), strip))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .relative()
                    // Clip a fixed-width sidebar instead of reflowing it, so rows do not
                    // re-wrap at every frame of the collapse.
                    .child(
                        div()
                            .flex_none()
                            .h_full()
                            .w(px(sidebar_now))
                            .overflow_hidden()
                            .child(sidebar),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(sidebar_now < SPACE_SM, |el| el.ml(px(SPACE_SM)))
                            .mr(px(SPACE_SM))
                            .mb(px(SPACE_SM))
                            .rounded(px(PANEL_RADIUS))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.terminal_background)
                            .overflow_hidden()
                            .child(self.panel_content(cx)),
                    )
                    .children(handle),
            )
            .when_some(self.picker.as_ref(), |el, p| {
                el.child(picker::render(
                    p,
                    &self.hosts,
                    window.viewport_size(),
                    &t,
                    cx,
                ))
            })
    }
}
