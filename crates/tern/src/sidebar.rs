// Adapted from zeron crates/ui/src/shell.rs (render_chat_row, sidebar footer) and
// crates/ui/src/shell/sidebar_sections.rs + spaces.rs (section header, project header) (MIT).
//! The host list, laid out as zeron's sidebar: a header with +, collapsible sections (tern's
//! own connections, then `~/.ssh/config`), one-line rows, and a footer with sync and settings.
//! Every row action lives in the right-click menu; hover shows a "…" that opens the same menu.

use crate::a11y::Accessible as _;
use crate::hover::HoverFade as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Transformation, div,
    percentage, px,
};
use tern_ssh::HostEntry;

use crate::connections::Connection;
use crate::icons::{self, icon};
use crate::session::Status;
use crate::shell::Shell;
use crate::tabs::{self, TabInfo};
use crate::text_input::TextInput;
use crate::theme::Theme;

pub const SAVED: &str = "Saved";
pub const RECENT: &str = "Recent";

/// `hosts[i]` is `connections[i].entry()`: the sidebar lists tern's own connections only.
pub struct SidebarState<'a> {
    pub hosts: &'a [HostEntry],
    pub connections: &'a [Connection],
    pub open: &'a [TabInfo],
    pub active_alias: Option<&'a str>,
    pub width: f32,
    pub collapsed: &'a [&'static str],
    pub collapsed_groups: &'a [String],
    /// Indices into `hosts`, newest first, already limited to what the section shows.
    pub recent: &'a [usize],
    pub query: &'a str,
    pub search: &'a Entity<TextInput>,
}

/// How the Saved section is laid out: ungrouped hosts first, then one folder per group.
#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    pub ungrouped: Vec<usize>,
    pub groups: Vec<(String, Vec<usize>)>,
}

/// Groups sorted by name (case-insensitive); hosts keep their saved order inside each.
pub fn layout(connections: &[Connection]) -> Layout {
    let mut ungrouped = Vec::new();
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (ix, c) in connections.iter().enumerate() {
        match c.group.as_deref() {
            None => ungrouped.push(ix),
            Some(name) => match groups.iter_mut().find(|(g, _)| g == name) {
                Some((_, rows)) => rows.push(ix),
                None => groups.push((name.to_owned(), vec![ix])),
            },
        }
    }
    groups.sort_by_key(|(name, _)| name.to_lowercase());
    Layout { ungrouped, groups }
}

/// The group names in use, for the form's suggestions.
pub fn group_names(connections: &[Connection]) -> Vec<String> {
    layout(connections)
        .groups
        .into_iter()
        .map(|(g, _)| g)
        .collect()
}

/// What the search matches a connection against: name, address, group and tags.
pub fn search_label(connection: &Connection) -> String {
    format!(
        "{} {} {} {}",
        connection.name,
        address(&connection.entry()),
        connection.group.as_deref().unwrap_or_default(),
        connection.tags.join(" ")
    )
}

/// Connections matching `query`, best first (the ranking the ⌘K picker uses).
pub fn search(query: &str, connections: &[Connection]) -> Vec<usize> {
    let labels: Vec<String> = connections.iter().map(search_label).collect();
    let names: Vec<&str> = connections.iter().map(|c| c.name.as_str()).collect();
    crate::picker::rank_named(query, &names, &labels)
}

pub fn render(
    s: &SidebarState<'_>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let mut list = div()
        .id("hosts")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px(px(8.));
    let one = |scope: &'static str, ix: usize, top: bool, cx: &mut Context<Shell>| {
        let host = &s.hosts[ix];
        let status = s
            .open
            .iter()
            .rev()
            .find(|tab| tab.alias == host.alias)
            .map(|tab| &tab.status);
        let active = s.active_alias == Some(host.alias.as_str());
        row(scope, ix, host, status, active, top, t, cx)
    };
    if !s.query.trim().is_empty() {
        let found = search(s.query, s.connections);
        let mut body = div().pt(px(8.)).flex().flex_col().gap(px(2.));
        for (n, &ix) in found.iter().enumerate() {
            body = body.child(one("found", ix, n == 0, cx));
        }
        list = list.child(body);
        if found.is_empty() {
            list = list.child(
                div()
                    .px(px(8.))
                    .pt(px(12.))
                    .text_size(px(12.))
                    .text_color(t.muted)
                    .child("No hosts match"),
            );
        }
    } else {
        if !s.recent.is_empty() {
            let collapsed = s.collapsed.contains(&RECENT);
            list = list.child(section_header(
                "recent-header",
                RECENT.into(),
                collapsed,
                false,
                t,
                cx,
                |shell, cx| shell.toggle_section(RECENT, cx),
            ));
            if !collapsed {
                let mut body = div().pt(px(4.)).flex().flex_col().gap(px(2.));
                for &ix in s.recent {
                    body = body.child(one("recent", ix, false, cx));
                }
                list = list.child(body);
            }
        }
        if !s.hosts.is_empty() {
            let collapsed = s.collapsed.contains(&SAVED);
            list = list.child(section_header(
                "saved-header",
                SAVED.into(),
                collapsed,
                false,
                t,
                cx,
                |shell, cx| shell.toggle_section(SAVED, cx),
            ));
            if !collapsed {
                let layout = layout(s.connections);
                let mut body = div().pt(px(4.)).flex().flex_col().gap(px(2.));
                for &ix in &layout.ungrouped {
                    body = body.child(one("host", ix, false, cx));
                }
                list = list.child(body);
                for (n, (name, rows)) in layout.groups.iter().enumerate() {
                    let folded = s.collapsed_groups.contains(name);
                    let toggled = name.clone();
                    list = list.child(section_header(
                        ("group-header", n),
                        SharedString::from(format!("{name} · {}", rows.len())),
                        folded,
                        true,
                        t,
                        cx,
                        move |shell, cx| shell.toggle_group(&toggled, cx),
                    ));
                    if !folded {
                        let mut body = div().pt(px(4.)).pl(px(12.)).flex().flex_col().gap(px(2.));
                        for &ix in rows {
                            body = body.child(one("host", ix, false, cx));
                        }
                        list = list.child(body);
                    }
                }
            }
        }
    }
    if s.hosts.is_empty() {
        list = list.child(empty_state(t, cx));
    }
    div()
        .w(px(s.width))
        .flex_none()
        .h_full()
        .min_h_0()
        .flex()
        .flex_col()
        .child(header(t, cx))
        .child(search_field(s.search, t, cx))
        .child(list)
        .child(footer(t, cx))
}

/// First run: no hosts, so say how to get some (the main panel shows the same two buttons).
fn empty_state(t: &Theme, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    div()
        .px(px(8.))
        .pt(px(12.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(12.))
                .text_color(t.muted)
                .child("No hosts yet."),
        )
        .child(
            div()
                .id("empty-new")
                .h(px(28.))
                .px(px(12.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(px(13.))
                .bg(t.accent)
                .text_color(gpui::white())
                .on_click(cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx)))
                .child("New connection"),
        )
        .child(
            div()
                .id("empty-import")
                .h(px(28.))
                .px(px(12.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(px(13.))
                .hover_fade("import-ssh-config", t.row_hover, t.row_active)
                .text_color(t.text)
                .on_click(cx.listener(|s, _, w, cx| s.open_import(w, cx)))
                .child("Import from ~/.ssh/config…"),
        )
}

/// zeron's `search_input_frame` (10 x 6 padding, 8 radius, 4 % ink) with a 13 pt magnifier.
fn search_field(
    input: &Entity<TextInput>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    div().px(px(8.)).pb(px(4.)).child(
        div()
            .px(px(10.))
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .bg(t.ink(0.04))
            .text_size(px(13.))
            .on_key_down(cx.listener(Shell::on_search_key))
            .child(icon(icons::SEARCH).size(px(13.)).text_color(t.muted))
            .child(div().flex_1().min_w_0().child(input.clone())),
    )
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
                .icon_button("New connection", t)
                .on_click(cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx))),
        )
}

/// A section label with zeron's collapse chevron (12 pt, rotated a quarter turn when open).
/// A group is the same header, indented.
fn section_header(
    id: impl Into<gpui::ElementId>,
    label: SharedString,
    collapsed: bool,
    nested: bool,
    t: &Theme,
    cx: &mut Context<Shell>,
    on_toggle: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let faint = t.muted.opacity(0.5);
    let id = id.into();
    let key = format!("section-{id}");
    div()
        .id(id)
        .mt(px(if nested { 4. } else { 12. }))
        .h(px(28.))
        .px(px(8.))
        .when(nested, |el| el.ml(px(12.)))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(8.))
        .cursor_pointer()
        .hover_fade(key, gpui::transparent_black(), t.row_hover)
        .on_click(cx.listener(move |s, _, _, cx| on_toggle(s, cx)))
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
        .child(div().text_size(px(12.)).text_color(faint).child(label))
}

/// The leading 13 pt slot: a 6 pt dot coloured by the host's newest tab.
fn status_dot(status: Option<&Status>, t: &Theme) -> impl IntoElement + use<> {
    let color: Option<Hsla> = status.map(|s| match s {
        Status::Idle => t.faint.opacity(0.8),
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

/// The "..." button's element id, distinct per section so a host listed twice has two.
fn scope_more(scope: &'static str) -> &'static str {
    match scope {
        "recent" => "recent-more",
        "found" => "found-more",
        _ => "host-more",
    }
}

/// One host on one line, as zeron's session row: dot, name, the address faint at the end.
#[allow(clippy::too_many_arguments)]
fn row(
    scope: &'static str,
    ix: usize,
    host: &HostEntry,
    status: Option<&Status>,
    active: bool,
    top: bool,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let target = host.clone();
    let menu_host = host.clone();
    let more_host = host.clone();
    let group: SharedString = format!("host-row-{scope}-{ix}").into();
    div()
        .id((scope, ix))
        .group(group.clone())
        .relative()
        .h(px(29.))
        .px(px(8.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .hover_fade(
            format!("host-{scope}-{ix}"),
            gpui::transparent_black(),
            t.ink(0.11),
        )
        .when(active || top, |el| el.bg(t.ink(0.11)))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.connect_host(target.clone(), window, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |shell, e: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                shell.open_host_menu(&menu_host, ix, e.position, cx);
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
                .id((scope_more(scope), ix))
                .icon_button("Host actions", t)
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
                    shell.open_host_menu(&more_host, ix, e.position(), cx);
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
                .hover_fade("footer-sync", gpui::transparent_black(), t.ink(0.09))
                .on_click(cx.listener(|s, _, w, cx| s.open_settings_at_sync(w, cx)))
                .child(icon(icons::CLOUD).size(px(16.)).text_color(t.muted))
                .child("Sync"),
        )
        .child(
            icon_button("footer-settings", icons::SETTINGS, 28., 15., t)
                .icon_button("Settings", t)
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
    let id = id.into();
    let key = format!("icon-button-{id}");
    div()
        .id(id)
        .flex_none()
        .size(px(box_size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover_fade(key, gpui::transparent_black(), t.ink(0.11))
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
            ..Default::default()
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

    fn conn(name: &str, group: Option<&str>, tags: &[&str]) -> Connection {
        Connection {
            name: name.into(),
            host: "10.0.0.5".into(),
            port: 22,
            user: "deploy".into(),
            identity_file: None,
            group: group.map(Into::into),
            tags: tags.iter().map(|t| (*t).into()).collect(),
            ..Connection::default()
        }
    }

    #[test]
    fn ungrouped_come_first_and_groups_sort_by_name_keeping_host_order() {
        let list = [
            conn("a", Some("Zeta"), &[]),
            conn("b", None, &[]),
            conn("c", Some("alpha"), &[]),
            conn("d", Some("Zeta"), &[]),
        ];
        let l = layout(&list);
        assert_eq!(l.ungrouped, vec![1]);
        assert_eq!(
            l.groups,
            vec![
                ("alpha".to_owned(), vec![2]),
                ("Zeta".to_owned(), vec![0, 3])
            ]
        );
    }

    #[test]
    fn search_matches_tags_and_group_and_ranks_the_name_first() {
        let list = [
            conn("billing", Some("prod"), &["eu", "postgres"]),
            conn("postgres-main", None, &[]),
            conn("web", Some("staging"), &["nginx"]),
        ];
        // A tag finds a host whose name lacks it.
        assert_eq!(search("nginx", &list), vec![2]);
        assert_eq!(search("staging", &list), vec![2]);
        // A name match and a tag-only match are both found; the name match comes first.
        let list2 = [
            conn("db", Some("prod"), &["web"]),
            conn("web-frontend", None, &[]),
        ];
        assert_eq!(search("web", &list2), vec![1, 0]);
        let mut found = search("postgres", &list);
        found.sort_unstable();
        assert_eq!(found, vec![0, 1]);
        assert_eq!(search("zzz", &list), Vec::<usize>::new());
        assert_eq!(search("", &list), vec![0, 1, 2]);
    }
}
