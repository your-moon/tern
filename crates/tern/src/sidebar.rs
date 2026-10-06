// Adapted from zeron crates/ui/src/shell.rs (render_chat_row, sidebar footer) and
// crates/ui/src/shell/sidebar_sections.rs + spaces.rs (section header, project header) (MIT).
//! The host list, laid out as zeron's sidebar: a header with +, collapsible sections (tern's
//! own connections, then `~/.ssh/config`), one-line rows, and a footer with sync and settings.
//! Every row action lives in the right-click menu; hover shows a "…" that opens the same menu.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Transformation, div, percentage, px,
};
use tern_ssh::HostEntry;

use crate::icons::{self, icon};
use crate::session::Status;
use crate::shell::Shell;
use crate::tabs::{self, TabInfo};
use crate::theme::Theme;

/// The first `count` hosts are tern's own connections (editable); the rest come from
/// `~/.ssh/config`.
#[derive(Debug, Clone, Copy)]
pub struct Editable {
    pub count: usize,
}

pub const SAVED: &str = "Saved";
pub const SSH_CONFIG: &str = "~/.ssh/config";

pub struct SidebarState<'a> {
    pub hosts: &'a [HostEntry],
    pub editable: Editable,
    pub open: &'a [TabInfo],
    pub active_alias: Option<&'a str>,
    pub width: f32,
    pub collapsed: &'a [&'static str],
}

pub fn render(
    s: &SidebarState<'_>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let (saved, config) = s.hosts.split_at(s.editable.count.min(s.hosts.len()));
    let mut list = div()
        .id("hosts")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px(px(8.));
    for (name, rows, offset) in [(SAVED, saved, 0), (SSH_CONFIG, config, saved.len())] {
        if rows.is_empty() {
            continue;
        }
        let collapsed = s.collapsed.contains(&name);
        list = list.child(section_header(name, collapsed, t, cx));
        if collapsed {
            continue;
        }
        let mut body = div().pt(px(4.)).flex().flex_col().gap(px(2.));
        for (i, host) in rows.iter().enumerate() {
            let ix = offset + i;
            let status = s
                .open
                .iter()
                .rev()
                .find(|tab| tab.alias == host.alias)
                .map(|tab| &tab.status);
            let editable = (ix < s.editable.count).then_some(ix);
            let active = s.active_alias == Some(host.alias.as_str());
            body = body.child(row(ix, host, status, active, editable, t, cx));
        }
        list = list.child(body);
    }
    if s.hosts.is_empty() {
        list = list.child(
            div()
                .px(px(8.))
                .pt(px(12.))
                .text_size(px(12.))
                .text_color(t.muted)
                .child("No hosts yet. Press + to add one."),
        );
    }
    div()
        .w(px(s.width))
        .flex_none()
        .h_full()
        .min_h_0()
        .flex()
        .flex_col()
        .child(header(t, cx))
        .child(list)
        .child(footer(t, cx))
}

/// zeron's project-header row: the title and a 24 pt + button.
fn header(t: &Theme, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    div()
        .px(px(16.))
        .pt(px(8.))
        .pb(px(4.))
        .h(px(37.))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(t.text.opacity(0.8))
                .child("Hosts"),
        )
        .child(
            icon_button("new-connection", icons::PLUS, 24., 16., t)
                .on_click(cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx))),
        )
}

/// A section label with zeron's collapse chevron (12 pt, rotated a quarter turn when open).
fn section_header(
    name: &'static str,
    collapsed: bool,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let faint = t.muted.opacity(0.5);
    div()
        .id(name)
        .mt(px(12.))
        .h(px(28.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(t.row_hover))
        .on_click(cx.listener(move |s, _, _, cx| s.toggle_section(name, cx)))
        .child(
            icon(icons::CHEVRON_RIGHT)
                .size(px(12.))
                .text_color(faint)
                .with_transformation(Transformation::rotate(percentage(if collapsed {
                    0.0
                } else {
                    0.25
                }))),
        )
        .child(div().text_size(px(12.)).text_color(faint).child(name))
}

/// The leading 13 pt slot: a 6 pt dot coloured by the host's newest tab.
fn status_dot(status: Option<&Status>, t: &Theme) -> impl IntoElement + use<> {
    let color: Option<Hsla> = status.map(|s| match s {
        Status::Connected => t.success.opacity(0.9),
        Status::Connecting => t.accent.opacity(0.6),
        Status::Closed => t.danger.opacity(0.65),
    });
    div()
        .flex_none()
        .size(px(13.))
        .flex()
        .items_center()
        .justify_center()
        .when_some(color, |el, c| {
            el.child(div().size(px(6.)).rounded_full().bg(c))
        })
}

/// One host on one line, as zeron's session row: dot, name, the address faint at the end.
fn row(
    ix: usize,
    host: &HostEntry,
    status: Option<&Status>,
    active: bool,
    editable: Option<usize>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let target = host.clone();
    let menu_host = host.clone();
    let more_host = host.clone();
    let group: SharedString = format!("host-row-{ix}").into();
    div()
        .id(("host", ix))
        .group(group.clone())
        .relative()
        .h(px(29.))
        .px(px(8.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .when(active, |el| el.bg(t.ink(0.11)))
        .when(!active, |el| el.hover(|s| s.bg(t.ink(0.11))))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.connect_host(target.clone(), window, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |shell, e: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                shell.open_host_menu(&menu_host, editable, e.position, cx);
            }),
        )
        .child(status_dot(status, t))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(13.))
                .line_height(px(17.))
                .text_color(if active { t.text } else { t.text.opacity(0.8) })
                .child(SharedString::from(tabs::middle_ellipsis(&host.alias, 26))),
        )
        .child(
            div()
                .flex_none()
                .max_w(px(110.))
                .truncate()
                .text_size(px(11.))
                .text_color(t.muted.opacity(0.5))
                .child(SharedString::from(short_address(host))),
        )
        // zeron's hover corner: floats over the end of the row, so it reserves no width.
        .child(
            div()
                .id(("host-more", ix))
                .absolute()
                .right(px(4.))
                .top(px(5.))
                .size(px(19.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .bg(t.popup)
                .invisible()
                .group_hover(group, |s| s.visible())
                .hover(|s| s.bg(t.ink(0.18)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |shell, e: &gpui::ClickEvent, _, cx| {
                    cx.stop_propagation();
                    shell.open_host_menu(&more_host, editable, e.position(), cx);
                }))
                .child(icon(icons::MORE).size(px(13.)).text_color(t.muted)),
        )
}

/// zeron's footer: a pill on the left (here: sync) and the settings gear on the right.
fn footer(t: &Theme, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    div()
        .px(px(8.))
        .py(px(8.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.))
        .child(
            div()
                .id("footer-sync")
                .h(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(t.text.opacity(0.8))
                .hover(|s| s.bg(t.ink(0.09)))
                .on_click(cx.listener(|s, _, w, cx| s.open_settings_at_sync(w, cx)))
                .child(icon(icons::CLOUD).size(px(16.)).text_color(t.muted))
                .child("Sync"),
        )
        .child(
            icon_button("footer-settings", icons::SETTINGS, 28., 15., t)
                .on_click(cx.listener(|s, _, w, cx| s.toggle_settings(w, cx))),
        )
}

pub fn icon_button(
    id: impl Into<gpui::ElementId>,
    path: &'static str,
    box_size: f32,
    icon_size: f32,
    t: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(box_size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(t.ink(0.11)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(icon(path).size(px(icon_size)).text_color(t.muted))
}

/// `user@host` with the port only when it is not 22, as `ssh` would be typed.
pub fn address(host: &HostEntry) -> String {
    let user = host
        .user
        .as_deref()
        .map(|u| format!("{u}@"))
        .unwrap_or_default();
    match host.port {
        22 => format!("{user}{}", host.host_name),
        port => format!("{user}{}:{port}", host.host_name),
    }
}

/// The trailing meta: just the host (and a non-default port), as the user is usually the same.
/// Empty when it would only repeat the name (configs that set HostName to the alias).
fn short_address(host: &HostEntry) -> String {
    let meta = match host.port {
        22 => host.host_name.clone(),
        port => format!("{}:{port}", host.host_name),
    };
    if meta == host.alias {
        String::new()
    } else {
        meta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(user: Option<&str>, port: u16) -> HostEntry {
        HostEntry {
            alias: "web".into(),
            host_name: "10.0.0.5".into(),
            port,
            user: user.map(Into::into),
            identity_files: Vec::new(),
            proxy_command: None,
        }
    }

    #[test]
    fn address_reads_like_an_ssh_target() {
        assert_eq!(address(&host(Some("deploy"), 22)), "deploy@10.0.0.5");
        assert_eq!(address(&host(None, 2222)), "10.0.0.5:2222");
        assert_eq!(short_address(&host(Some("deploy"), 2222)), "10.0.0.5:2222");
        let mut same = host(None, 22);
        same.host_name = "web".into();
        assert_eq!(short_address(&same), "");
    }
}
