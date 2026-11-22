// Adapted from zeron crates/ui/src/shell.rs (render_chat_row, sidebar footer) and
// crates/ui/src/shell/sidebar_sections.rs + spaces.rs (section header, project header) (MIT).
//! Connection workspaces with expandable session lists, user-defined folders, search and a footer.
//! Every row action lives in the right-click menu; hover shows a "…" that opens the same menu.

use crate::hover::Accessible as _;
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
use crate::indicator::Dot;
use crate::shell::Shell;
use crate::tabs::TabInfo;
use crate::text_input::TextInput;
use crate::theme::Theme;

/// Saved hosts have the same indices as their connections. Quick-connect sessions can have other aliases.
pub struct SidebarState<'a> {
    pub hosts: &'a [HostEntry],
    pub connections: &'a [Connection],
    pub open: &'a [TabInfo],
    pub active_alias: Option<&'a str>,
    pub active_index: Option<usize>,
    pub collapsed_connections: &'a [String],
    pub width: f32,
    pub collapsed_groups: &'a [String],
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

/// First tab of each open connection which has no saved host row. Matches use the same
/// fuzzy ranking as saved hosts. With no search the first tab sets the group's position.
pub fn unsaved_matches(query: &str, hosts: &[HostEntry], open: &[TabInfo]) -> Vec<usize> {
    let mut seen: std::collections::HashSet<&str> =
        hosts.iter().map(|host| host.alias.as_str()).collect();
    let first: Vec<usize> = open
        .iter()
        .enumerate()
        .filter_map(|(ix, tab)| seen.insert(tab.alias.as_str()).then_some(ix))
        .collect();
    if query.trim().is_empty() {
        return first;
    }
    let labels: Vec<&str> = first.iter().map(|ix| open[*ix].alias.as_str()).collect();
    crate::picker::rank_labels(query, &labels)
        .into_iter()
        .map(|ix| first[ix])
        .collect()
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
    let mut sessions: std::collections::HashMap<&str, Vec<usize>> =
        std::collections::HashMap::new();
    for (ix, tab) in s.open.iter().enumerate() {
        sessions.entry(&tab.alias).or_default().push(ix);
    }
    let unsaved = unsaved_matches(s.query, s.hosts, s.open);
    let one = |scope, ix: usize, top, cx: &mut Context<Shell>| {
        let host = &s.hosts[ix];
        connection_rows(
            scope,
            ix,
            &host.alias,
            Some(host),
            sessions.get(host.alias.as_str()).map_or(&[], Vec::as_slice),
            top,
            s,
            t,
            cx,
        )
    };
    if !s.query.trim().is_empty() {
        let found = search(s.query, s.connections);
        let mut body = div().pt(px(8.)).flex().flex_col().gap(px(2.));
        for (n, &ix) in found.iter().enumerate() {
            body = body.child(one("found", ix, n == 0, cx));
        }
        list = list.child(body);
        if found.is_empty() && unsaved.is_empty() {
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
        // One list: tern's connections, ungrouped first, then each group under its own header.
        // No Recent (it repeated these rows) and no wrapper section around them; recency orders
        // the ⌘K picker instead.
        if !s.hosts.is_empty() {
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
    for ix in unsaved {
        let alias = &s.open[ix].alias;
        list = list.child(connection_rows(
            "open-host",
            ix,
            alias,
            None,
            &sessions[alias.as_str()],
            false,
            s,
            t,
            cx,
        ));
    }
    if s.hosts.is_empty() && s.open.is_empty() {
        list = list.child(empty_state(t, cx));
    }
    div()
        .w(px(s.width))
        .flex_none()
        .h_full()
        .min_h_0()
        .flex()
        .flex_col()
        .child(header(s.active_alias.is_none(), t, cx))
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
        .child(crate::empty_state::view(
            t,
            crate::empty_state::HOSTS,
            "No hosts yet.",
        ))
        .child(
            crate::settings_widgets::action_button(t, "empty-new", "New connection", true)
                .on_click(cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx))),
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
            .border_1()
            .border_color(t.hairline)
            .bg(t.field_fill())
            .text_size(px(13.))
            .on_key_down(cx.listener(Shell::on_search_key))
            .child(icon(icons::SEARCH).size(px(13.)).text_color(t.muted))
            .child(div().flex_1().min_w_0().child(input.clone())),
    )
}

/// zeron's project-header row: the title and a 24 pt + button.
fn header(selected: bool, t: &Theme, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    div()
        .px(px(8.))
        .py(px(6.))
        .flex()
        .items_center()
        .gap(px(4.))
        .child(
            div()
                .id("connections-overview")
                .flex_1()
                .min_w_0()
                .h(px(32.))
                .px(px(8.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .when(selected, |el| el.bg(t.row_active))
                .hover(|el| el.bg(t.row_hover))
                .on_click(cx.listener(|s, _, w, cx| s.show_connections(w, cx)))
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(t.text)
                .child(icon(icons::HOME).size(px(16.)).text_color(t.muted))
                .child("Connections"),
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
    let faint = t.muted.opacity(crate::theme::by_dpi(0.7, 1.0));
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

/// The session status dot, in a fixed slot so session titles line up.
pub(crate) fn status_dot(status: Option<Dot>, t: &Theme) -> impl IntoElement + use<> {
    let color: Option<(Hsla, bool)> = status.map(|d| {
        let alpha = match d {
            Dot::Idle => 0.8,
            Dot::Connected => 0.9,
            Dot::Connecting => 0.6,
            Dot::WaitingForInput => 0.9,
            Dot::Closed => 0.65,
        };
        (d.color(t).opacity(alpha), d.hollow())
    });
    div()
        .flex_none()
        .size(px(13.))
        .flex()
        .items_center()
        .justify_center()
        .when_some(color, |el, (c, hollow)| {
            let disc = div().size(px(crate::theme::fit(13., 6.))).rounded_full();
            el.child(if hollow {
                disc.border_1().border_color(c)
            } else {
                disc.bg(c)
            })
        })
}

/// Connection header and its visible sessions. Session actions retain their global tab indices.
#[allow(clippy::too_many_arguments)]
fn connection_rows(
    scope: &'static str,
    ix: usize,
    alias: &str,
    host: Option<&HostEntry>,
    sessions: &[usize],
    top: bool,
    state: &SidebarState<'_>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let target = alias.to_owned();
    let folded = state.collapsed_connections.iter().any(|name| name == alias);
    let only_session = match sessions {
        [ix] if state.open[*ix].title.is_none() => Some(&state.open[*ix]),
        _ => None,
    };
    let show_children = !sessions.is_empty() && only_session.is_none();
    let active = state.active_alias == Some(alias);
    let group: SharedString = format!("host-row-{scope}-{ix}").into();
    let meta_size = if crate::theme::low_dpi() { 12. } else { 11. };
    let meta = host.and_then(|host| fit_meta(host, state.width - 61., meta_size));
    let header =
        div()
            .id((scope, ix))
            .group(group.clone())
            .relative()
            .h(px(if meta.is_some() { 44. } else { 32. }))
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
            .when(top || (active && (folded || !show_children)), |el| {
                el.bg(t.row_active)
            })
            .on_click(cx.listener(move |shell, _, window, cx| {
                shell.select_connection(&target, window, cx);
            }))
            .child(if !show_children {
                div()
                    .size(px(15.))
                    .flex_none()
                    .child(status_dot(only_session.map(|tab| tab.dot), t))
            } else {
                let toggled = alias.to_owned();
                div().size(px(15.)).flex_none().child(
                    div()
                        .id("connection-disclosure")
                        .icon_button(
                            if folded {
                                "Show sessions"
                            } else {
                                "Hide sessions"
                            },
                            t,
                        )
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            cx.stop_propagation();
                            shell.toggle_connection_group(&toggled, cx);
                        }))
                        .child(
                            icon(icons::CHEVRON_RIGHT)
                                .size(px(12.))
                                .text_color(t.muted)
                                .with_transformation(Transformation::rotate(percentage(
                                    if folded { 0. } else { 0.25 },
                                ))),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(17.))
                            .text_color(t.text)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(alias.to_owned()),
                    )
                    .when_some(meta.clone(), |el, text| {
                        el.child(
                            div()
                                .truncate()
                                .text_size(px(meta_size))
                                .line_height(px(14.))
                                .text_color(t.muted)
                                .child(text),
                        )
                    }),
            )
            .when(host.is_none() && only_session.is_some(), |el| {
                el.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |shell, e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        shell.open_tab_menu(ix, e.position, cx);
                    }),
                )
            })
            .when_some(host, |el, host| {
                let menu_host = host.clone();
                let more_host = host.clone();
                el.when(
                    meta.as_deref() != Some(short_address(host).as_str()),
                    |el| el.hint(address(host), t),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |shell, e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        shell.open_host_menu(&menu_host, ix, e.position, cx);
                    }),
                )
                .child(
                    div()
                        .id("host-more")
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
                        .child(
                            icon(icons::MORE)
                                .size(px(crate::theme::fit(19., 13.)))
                                .text_color(t.muted),
                        ),
                )
            });
    let mut body = div().flex_none().flex().flex_col().child(header);
    if show_children && !folded {
        for (ordinal, &ix) in sessions.iter().enumerate() {
            let tab = &state.open[ix];
            body = body.child(
                div()
                    .id(("connection-session", ix))
                    .relative()
                    .ml(px(9.))
                    .pl(px(20.))
                    .h(px(30.))
                    .pr(px(8.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .hover_fade(
                        format!("connection-session-{ix}"),
                        gpui::transparent_black(),
                        t.row_hover,
                    )
                    .when(active && state.active_index == Some(ix), |el| {
                        el.bg(t.row_active)
                    })
                    .on_click(
                        cx.listener(move |shell, _, window, cx| shell.show_tab(ix, window, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |shell, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            shell.open_tab_menu(ix, e.position, cx);
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top_0()
                            .w(px(8.))
                            .h(px(15.))
                            .border_l_1()
                            .border_b_1()
                            .rounded_bl(px(6.))
                            .border_color(t.border),
                    )
                    .when(ordinal + 1 < sessions.len(), |el| {
                        el.child(
                            div()
                                .absolute()
                                .left(px(6.))
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(t.border),
                        )
                    })
                    .child(status_dot(Some(tab.dot), t))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(t.text)
                            .child(tab.session_label(ordinal + 1)),
                    ),
            );
        }
    }
    body
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
                .text_color(t.text.opacity(crate::theme::by_dpi(0.8, 0.9)))
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
        .child(
            icon(path)
                .size(px(crate::theme::fit(box_size, icon_size)))
                .text_color(t.muted),
        )
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

/// The meta that fits in a row `inner` px wide at `size` px text: the address, else the host
/// without its port, else nothing. An address is never cut, so an IP never loses an octet; the
/// full text stays in the row's hover tip. Digits in Geist run about 0.6 em.
fn fit_meta(host: &HostEntry, inner: f32, size: f32) -> Option<String> {
    let fits = |s: &str| s.chars().count() as f32 * size * 0.6 <= inner;
    let full = short_address(host);
    if full.is_empty() {
        return None;
    }
    if fits(&full) {
        return Some(full);
    }
    (host.host_name != host.alias && fits(&host.host_name)).then(|| host.host_name.clone())
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
#[path = "sidebar_tests.rs"]
mod tests;
