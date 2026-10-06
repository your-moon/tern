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

use crate::connections::{self, Connection};
use crate::pane::{self, DragGhost, SidebarResize, WidthTween};
use crate::picker::{self, Picker, ToggleHostPicker};
use crate::session::{Launch, Session, Status};
use crate::settings::{self, SIDEBAR_DEFAULT, Settings};
use crate::split::{self, PaneId};
use crate::tabs::{self, ActivateTab, CloseTab, NextTab, PrevTab, TabInfo};
use crate::theme::{PANEL_RADIUS, SPACE_SM, Theme, UI_FONT};
use crate::{sidebar, statusline, titlebar};

actions!(
    tern,
    [
        ToggleSidebar,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        NewConnection,
        SplitRight,
        SplitDown,
        FocusPaneLeft,
        FocusPaneRight,
        FocusPaneUp,
        FocusPaneDown,
        OpenSettings
    ]
);

#[path = "shell_broadcast.rs"]
mod broadcast_ui;
#[path = "shell_look.rs"]
mod look;
#[path = "shell_menu.rs"]
mod menu;
#[path = "shell_open.rs"]
mod open;
#[path = "shell_panes.rs"]
mod panes_ui;
#[path = "shell_settings.rs"]
mod settings_ui;
#[path = "shell_sync.rs"]
mod sync_ui;
#[path = "shell_tabs.rs"]
mod tabs_ui;
#[path = "shell_term_options.rs"]
mod term_options;
#[path = "shell_toast.rs"]
mod toast;
#[path = "shell_update.rs"]
mod update_ui;
#[path = "shell_vault_page.rs"]
mod vault_page_ui;
#[path = "shell_vault.rs"]
mod vault_ui;
#[path = "shell_wallpaper.rs"]
mod wallpaper_ui;

pub(crate) use toast::CubicBezier;

pub(crate) use toast::{Kind as ToastKind, Toast};
#[path = "shell_themes.rs"]
mod themes_ui;

pub(crate) use themes_ui::ThemeTarget;

#[path = "shell_connections.rs"]
mod connections_ui;
#[path = "shell_hostlist.rs"]
mod hostlist;
#[path = "shell_import.rs"]
mod import_ui;
#[path = "shell_snippets.rs"]
mod snippets_ui;

pub(crate) use hostlist::FocusHostSearch;
pub(crate) use snippets_ui::ToggleSnippets;

/// 1320×880; debug builds take `TERN_DEV_WINDOW=900x600` so a scripted check can open the
/// window at its minimum.
fn start_size() -> (f32, f32) {
    #[cfg(debug_assertions)]
    if let Some((w, h)) = std::env::var("TERN_DEV_WINDOW")
        .ok()
        .and_then(|v| v.split_once('x').map(|(w, h)| (w.parse(), h.parse())))
        .and_then(|(w, h)| Some((w.ok()?, h.ok()?)))
    {
        return (w, h);
    }
    (1320., 880.)
}

pub fn open_main_window(cx: &mut App) -> anyhow::Result<WindowHandle<Shell>> {
    let (w, h) = start_size();
    let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
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
    let recent = settings::dir()
        .map(|d| crate::recent::Recent::load(&d))
        .unwrap_or_default();
    let (connections, store_error) = match settings::dir().map(|d| connections::load(&d)) {
        Some(Ok(list)) => (list, None),
        Some(Err(e)) => {
            tracing::warn!(error = %e, "hosts_json_unreadable");
            (Vec::new(), Some(e.to_string()))
        }
        None => (Vec::new(), None),
    };
    let window = cx.open_window(options, |_, cx| {
        cx.new(|cx| {
            let theme = Theme::zeron_dark();
            let mut shell = Shell {
                focus: cx.focus_handle(),
                settings: settings::dir()
                    .map(|d| Settings::load(&d))
                    .unwrap_or_default(),
                theme,
                hosts: Vec::new(),
                connections,
                store_error,
                form: None,
                confirm_delete: None,
                tabs: Vec::new(),
                renaming: None,
                restoring: false,
                next_tab: 0,
                next_pane: 0,
                broadcast: None,
                broadcast_picker: None,
                active: 0,
                error: None,
                picker: None,
                sidebar_tween: None,
                settings_page: None,
                theme_picker: None,
                recording: None,
                record_notice: None,
                record_interceptor: None,
                sync_ui: None,
                auto_sync: sync_ui::AutoSync::default(),
                vault_ui: None,
                last_input: std::time::Instant::now(),
                toasts: toast::Toasts::new(),
                tab_scroll: gpui::ScrollHandle::new(),
                context_menu: None,
                collapsed_sections: Vec::new(),
                hostlist: hostlist::HostList::new(&theme, recent, cx),
                snippets: snippets_ui::SnippetsUi::load(),
                import: None,
                wp: wallpaper_ui::State::default(),
                system_light: false,
                _clock: None,
            };
            shell.refresh_hosts();
            shell
        })
    })?;
    window.update(cx, |shell, window, cx| {
        crate::motion::apply(shell.settings.reduce_motion, cx);
        // macOS posts no notification gpui forwards, so re-read the preference whenever tern
        // comes to the front: the user changes it in System Settings, then switches back.
        cx.observe_window_activation(window, |shell, _, cx| {
            crate::motion::apply(shell.settings.reduce_motion, cx);
        })
        .detach();
        shell.system_light = is_light(window.appearance());
        shell.apply_appearance(cx);
        shell.install_input_colors(cx);
        shell.start_clock(cx);
        cx.observe_window_appearance(window, |shell, window, cx| {
            shell.system_light = is_light(window.appearance());
            shell.apply_appearance(cx);
        })
        .detach();
        shell.check_for_updates(cx);
        shell.start_auto_sync(window, cx);
    })?;
    // With no tab open nothing else holds focus, and gpui only dispatches key bindings along
    // the focused element's path, so the shell itself must be focused for ⌘K to work.
    window.update(cx, |shell, window, cx| window.focus(&shell.focus, cx))?;
    window.update(cx, |shell, window, cx| shell.restore_tabs(window, cx))?;
    window.update(cx, |shell, _, cx| shell.watch_idle(cx))?;
    Ok(window)
}

/// macOS's light appearances; the dark ones and anything new count as dark.
fn is_light(appearance: gpui::WindowAppearance) -> bool {
    matches!(
        appearance,
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
    )
}

pub struct Shell {
    focus: FocusHandle,
    settings: Settings,
    theme: Theme,
    /// What the sidebar and picker list: tern's own connections, in `hosts.json` order.
    hosts: Vec<HostEntry>,
    connections: Vec<Connection>,
    /// Set when `hosts.json` exists but cannot be read; saving is refused so it is not lost.
    store_error: Option<String>,
    form: Option<connections_ui::ConnectionForm>,
    confirm_delete: Option<usize>,
    tabs: Vec<Tab>,
    /// The tab whose title is being edited in the strip.
    renaming: Option<tabs_ui::Rename>,
    /// True while the last run's tabs are being reopened, so half a list is never saved.
    restoring: bool,
    /// Input typed in one tab also going to others, and the list that picks them.
    next_tab: broadcast_ui::TabId,
    next_pane: PaneId,
    broadcast: Option<broadcast_ui::Broadcast>,
    broadcast_picker: Option<broadcast_ui::BroadcastPicker>,
    active: usize,
    error: Option<String>,
    picker: Option<Picker>,
    sidebar_tween: Option<WidthTween>,
    settings_page: Option<settings_ui::Section>,
    theme_picker: Option<themes_ui::ThemePicker>,
    /// The shortcut being recorded in Settings → Shortcuts, and why the last one was refused.
    recording: Option<crate::keymap::ShortcutId>,
    record_notice: Option<String>,
    record_interceptor: Option<Subscription>,
    sync_ui: Option<sync_ui::SyncUi>,
    auto_sync: sync_ui::AutoSync,
    vault_ui: Option<vault_ui::VaultUi>,
    /// The last key or mouse input to the window, for the vault's idle lock.
    last_input: std::time::Instant,
    toasts: toast::Toasts,
    tab_scroll: gpui::ScrollHandle,
    context_menu: Option<menu::ContextMenu>,
    collapsed_sections: Vec<&'static str>,
    hostlist: hostlist::HostList,
    snippets: snippets_ui::SnippetsUi,
    import: Option<import_ui::ImportSheet>,
    wp: wallpaper_ui::State,
    /// Whether macOS is in its light appearance; what `Appearance: System` follows.
    system_light: bool,
    _clock: Option<gpui::Task<()>>,
}

/// Debug builds: keep scripted logins out of the real ~/.ssh/known_hosts.
#[cfg(debug_assertions)]
fn with_dev_known_hosts(spec: ConnectSpec) -> ConnectSpec {
    ConnectSpec {
        known_hosts: std::env::var_os("TERN_KNOWN_HOSTS")
            .map(Into::into)
            .or(spec.known_hosts),
        ..spec
    }
}

/// One terminal in a tab: a connection.
struct Pane {
    id: PaneId,
    session: Entity<Session>,
    _repaint: Subscription,
    _bell: Subscription,
}

struct Tab {
    id: broadcast_ui::TabId,
    alias: String,
    /// The user's name for the tab. It belongs to the tab, not the session, so a reconnect
    /// keeps it.
    title: Option<String>,
    /// How the panes are laid out, and which one has the keyboard.
    tree: split::Node,
    focused: PaneId,
    panes: Vec<Pane>,
}

impl Tab {
    /// The focused pane's session: what the strip shows and what typing reaches.
    fn session(&self) -> &Entity<Session> {
        match self.panes.iter().find(|p| p.id == self.focused) {
            Some(pane) => &pane.session,
            // `focused` always names a pane; the first is the fallback if that ever breaks.
            None => &self.panes[0].session,
        }
    }
}

impl Shell {
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
        if self.picker.take().is_some() {
            self.restore_focus(window, cx);
        }
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

    fn refresh_hosts(&mut self) {
        self.hosts = self.connections.iter().map(Connection::entry).collect();
    }

    pub(crate) fn toggle_section(&mut self, name: &'static str, cx: &mut Context<Self>) {
        if let Some(at) = self.collapsed_sections.iter().position(|n| *n == name) {
            self.collapsed_sections.remove(at);
        } else {
            self.collapsed_sections.push(name);
        }
        cx.notify();
    }

    /// Focus back to the active terminal, or the window when there is none.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = match self.tabs.get(self.active) {
            Some(tab) => tab.session().read(cx).view.focus_handle(cx),
            None => self.focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    fn apply_option_as_meta(&self, cx: &mut Context<Self>) {
        let on = self.settings.option_as_meta;
        for tab in &self.tabs {
            for pane in &tab.panes {
                let view = pane.session.read(cx).view.clone();
                view.update(cx, |v, _| v.set_option_as_meta(on));
            }
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
            .enumerate()
            .map(|(ix, tab)| TabInfo {
                alias: tab.alias.clone(),
                title: tab.title.clone(),
                logging: tab.session().read(cx).is_logging(),
                broadcast: self.is_broadcasting(tab.id),
                status: tab.session().read(cx).status.clone(),
                rename: self
                    .renaming
                    .as_ref()
                    .filter(|r| r.ix == ix)
                    .map(|r| r.input.clone()),
            })
            .collect()
    }

    fn panel_content(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(tab) = self.tabs.get(self.active) {
            return self.render_panes(tab, cx);
        }
        let t = self.theme;
        let keymap = cx.global::<crate::keymap::Keymap>();
        let hint = |id: crate::keymap::ShortcutId, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .min_w(px(44.))
                        .flex()
                        .justify_end()
                        .text_sm()
                        .text_color(t.text)
                        .child(SharedString::from(crate::keymap::badge(keymap.combo(id)))),
                )
                .child(div().text_sm().text_color(t.muted).child(label))
        };
        let (title, detail): (SharedString, Option<SharedString>) = match &self.error {
            Some(e) => ("Could not open that".into(), Some(e.clone().into())),
            None if self.hosts.is_empty() => (
                "No hosts yet".into(),
                Some("Add a connection, or copy your hosts over from ~/.ssh/config".into()),
            ),
            None => ("No session open".into(), None),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(div().text_size(px(15.)).text_color(t.text).child(title))
                    .when_some(detail, |el, d| {
                        el.child(div().text_sm().text_color(t.muted).child(d))
                    }),
            )
            .when(self.hosts.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            connections_ui::button("empty-main-new", "New connection", true, &t)
                                .on_click(
                                    cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx)),
                                ),
                        )
                        .child(
                            connections_ui::button(
                                "empty-main-import",
                                "Import from ~/.ssh/config…",
                                false,
                                &t,
                            )
                            .on_click(cx.listener(|s, _, w, cx| s.open_import(w, cx))),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(hint(crate::keymap::ShortcutId::HostPicker, "Find a host"))
                    .child(hint(
                        crate::keymap::ShortcutId::NewConnection,
                        "New connection",
                    ))
                    .child(hint(crate::keymap::ShortcutId::Settings, "Settings")),
            )
            .into_any_element()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Whatever held focus can disappear (a field hidden after sync, a closed form or tab);
        // with nothing focused gpui dispatches no keys at all, so the window takes it back.
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        let t = self.theme;
        let infos = self.tab_infos(cx);
        let active_alias = infos.get(self.active).map(|i| i.alias.clone());
        let strip = tabs::strip(&infos, self.active, &self.tab_scroll, &t, cx);
        let query = self.search_query(cx);
        let recent_rows = self.recent_indices();
        let sidebar = sidebar::render(
            &sidebar::SidebarState {
                hosts: &self.hosts,
                connections: &self.connections,
                open: &infos,
                active_alias: active_alias.as_deref(),
                width: self.settings.sidebar_width,
                collapsed: &self.collapsed_sections,
                collapsed_groups: &self.hostlist.collapsed_groups,
                recent: &recent_rows,
                query: &query,
                search: &self.hostlist.search,
            },
            &t,
            cx,
        );
        let form = self.render_form(window, cx);
        let import = self.render_import(window, cx);
        let snippet_picker = self.render_snippet_picker(window, cx);
        let snippet_form = self.render_snippet_form(window, cx);
        self.sync_wallpaper(cx);
        let wallpaper = self.has_wallpaper();
        let has_tab = !self.tabs.is_empty();
        let panel_bg = self.panel_background();
        let panel_bg = if wallpaper {
            panel_bg.opacity(crate::theme::GLASS_ALPHA)
        } else {
            panel_bg
        };
        let layers = self.wallpaper_layers(window);
        let status_line = self.render_status_line(panel_bg, cx);
        let theme_picker = self.render_theme_picker(window, cx);
        let toast = self.render_toast(window, cx);
        let context_menu = self.render_menu(cx);
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
        let root = div()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|s, _, _, _| s.last_input = std::time::Instant::now()))
            .capture_any_mouse_down(
                cx.listener(|s, _, _, _| s.last_input = std::time::Instant::now()),
            )
            .on_mouse_move(cx.listener(|s, _, _, _| s.last_input = std::time::Instant::now()))
            .on_scroll_wheel(cx.listener(|s, _, _, _| s.last_input = std::time::Instant::now()))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(t.glass())
            .children(layers)
            .font_family(UI_FONT)
            .text_color(t.text)
            .on_action(cx.listener(|s, _: &CloseTab, w, cx| s.close_pane_or_tab(w, cx)))
            .on_action(
                cx.listener(|s, _: &SplitRight, w, cx| s.split_pane(split::Axis::Row, w, cx)),
            )
            .on_action(
                cx.listener(|s, _: &SplitDown, w, cx| s.split_pane(split::Axis::Column, w, cx)),
            )
            .on_action(cx.listener(|s, _: &FocusPaneLeft, w, cx| {
                s.move_pane_focus(split::Direction::Left, w, cx)
            }))
            .on_action(cx.listener(|s, _: &FocusPaneRight, w, cx| {
                s.move_pane_focus(split::Direction::Right, w, cx)
            }))
            .on_action(cx.listener(|s, _: &FocusPaneUp, w, cx| {
                s.move_pane_focus(split::Direction::Up, w, cx)
            }))
            .on_action(cx.listener(|s, _: &FocusPaneDown, w, cx| {
                s.move_pane_focus(split::Direction::Down, w, cx)
            }))
            .on_action(cx.listener(|s, _: &NextTab, w, cx| {
                s.activate_tab(tabs::step(s.active, s.tabs.len(), 1), w, cx)
            }))
            .on_action(cx.listener(|s, _: &PrevTab, w, cx| {
                s.activate_tab(tabs::step(s.active, s.tabs.len(), -1), w, cx)
            }))
            .on_action(cx.listener(|s, a: &ActivateTab, w, cx| s.activate_tab(a.0, w, cx)))
            .on_action(cx.listener(|s, _: &ToggleHostPicker, w, cx| s.toggle_picker(w, cx)))
            .on_action(cx.listener(|s, _: &FocusHostSearch, w, cx| s.focus_search(w, cx)))
            .on_action(cx.listener(|s, _: &ToggleSnippets, w, cx| s.toggle_snippet_picker(w, cx)))
            .on_action(cx.listener(|s, _: &ToggleSidebar, _, cx| s.toggle_sidebar(cx)))
            .on_action(cx.listener(|s, _: &NewConnection, w, cx| s.open_form(None, None, w, cx)))
            .on_action(cx.listener(|s, _: &OpenSettings, w, cx| s.toggle_settings(w, cx)))
            // Capture phase: the terminal handles Escape itself, and ending a broadcast must
            // not depend on which pane has focus.
            .capture_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape"
                    && e.keystroke.modifiers == gpui::Modifiers::default()
                    && s.on_broadcast_escape(cx)
                {
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, w, cx| {
                if s.on_rename_key(e, w, cx) {
                    cx.stop_propagation();
                    return;
                }
                if e.keystroke.key == "escape" && s.context_menu.is_some() {
                    s.context_menu = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if e.keystroke.key == "escape" && s.settings_page.is_some() && s.form.is_none() {
                    s.toggle_settings(w, cx);
                    cx.stop_propagation();
                }
            }))
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
            .child(titlebar::render(
                &t,
                window.is_fullscreen(),
                !self.settings.sidebar_collapsed,
                strip,
                cx,
            ))
            .child(match self.settings_page {
                Some(section) => self.render_settings(section, cx),
                None => div()
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
                            // With a wallpaper the terminal view paints its own translucent
                            // fill; a second one here would stack.
                            .when(!(wallpaper && has_tab), |el| el.bg(panel_bg))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .child(div().flex_1().min_h_0().child(self.panel_content(cx)))
                            .when_some(status_line, |el, line| el.child(line)),
                    )
                    .children(handle)
                    .into_any_element(),
            })
            .when_some(form, |el, form| el.child(form))
            .when_some(import, |el, i| el.child(i))
            .when_some(snippet_picker, |el, p| el.child(p))
            .when_some(snippet_form, |el, f| el.child(f))
            .when_some(theme_picker, |el, p| el.child(p))
            .when_some(context_menu, |el, m| el.child(m))
            .when_some(self.render_broadcast_picker(cx), |el, p| el.child(p))
            .when_some(toast, |el, t| el.child(t))
            .when_some(self.picker.as_ref(), |el, p| {
                el.child(picker::render(
                    p,
                    &self.hosts,
                    &self.picker_matches(),
                    &self.recent_aliases(),
                    window.viewport_size(),
                    &t,
                    cx,
                ))
            });
        // Another frame while any hover fade is mid-flight (zeron drives it the same way).
        if crate::hover::fades_active() {
            window.request_animation_frame();
        }
        root
    }
}
