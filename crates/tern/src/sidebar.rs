// Adapted from zeron crates/ui/src/shell.rs (sidebar rows: active/hover washes) (MIT).
//! The host list: every concrete `Host` in `~/.ssh/config`; clicking one connects to it.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};
use tern_ssh::HostEntry;

use crate::session::Status;
use crate::shell::Shell;
use crate::tabs::{self, TabInfo};
use crate::theme::{CONTROL_RADIUS, SPACE_SM, Theme};

/// The first `count` hosts are tern's own connections: they can be edited and deleted. The
/// rest come from `~/.ssh/config` and can only be duplicated into an editable copy.
#[derive(Debug, Clone, Copy)]
pub struct Editable {
    pub count: usize,
    pub confirm_delete: Option<usize>,
}

/// `open` are the tabs; a host shows the status of its newest tab and is highlighted when
/// that tab is the active one.
pub fn render(
    hosts: &[HostEntry],
    editable: Editable,
    open: &[TabInfo],
    active_alias: Option<&str>,
    width: f32,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let mut rows = Vec::with_capacity(hosts.len());
    for (ix, host) in hosts.iter().enumerate() {
        let status = open
            .iter()
            .rev()
            .find(|tab| tab.alias == host.alias)
            .map(|tab| &tab.status);
        let active = active_alias == Some(host.alias.as_str());
        if ix == editable.count && ix > 0 {
            rows.push(section("~/.ssh/config", t).into_any_element());
        }
        let kind = if ix < editable.count {
            RowKind::Connection {
                confirming: editable.confirm_delete == Some(ix),
            }
        } else {
            RowKind::SshConfig
        };
        rows.push(row(ix, host, status, active, kind, t, cx).into_any_element());
    }
    div()
        .w(px(width))
        .flex_none()
        .h_full()
        .min_h_0()
        .flex()
        .flex_col()
        .px(px(SPACE_SM))
        .child(
            div()
                .px(px(SPACE_SM))
                .py(px(6.))
                .flex()
                .items_center()
                .justify_between()
                .text_xs()
                .text_color(t.faint)
                .child("Hosts")
                .child(icon_button("new-connection", "+", t).on_click(
                    cx.listener(|shell, _, window, cx| shell.open_form(None, None, window, cx)),
                )),
        )
        .child(
            div()
                .id("hosts")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(rows)
                .when(hosts.is_empty(), |el| {
                    el.child(
                        div()
                            .px(px(SPACE_SM))
                            .text_xs()
                            .text_color(t.muted)
                            .child("No hosts yet. Press + to add one."),
                    )
                }),
        )
}

#[derive(Debug, Clone, Copy)]
enum RowKind {
    Connection { confirming: bool },
    SshConfig,
}

fn section(label: &'static str, t: &Theme) -> impl IntoElement + use<> {
    div()
        .px(px(SPACE_SM))
        .pt(px(10.))
        .pb(px(4.))
        .text_xs()
        .text_color(t.faint)
        .child(label)
}

fn icon_button(
    id: impl Into<gpui::ElementId>,
    glyph: &'static str,
    t: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .text_color(t.muted)
        .cursor_pointer()
        .hover(|s| s.bg(t.row_hover).text_color(t.text))
        // A click on a row's button must not also connect the row.
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(glyph)
}

/// Hover actions: edit and delete for a connection, duplicate for a `~/.ssh/config` host.
fn actions(
    ix: usize,
    host: &HostEntry,
    kind: RowKind,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let bar = div()
        .flex_none()
        .flex()
        .gap(px(2.))
        .invisible()
        .group_hover("host-row", |s| s.visible());
    match kind {
        RowKind::Connection { confirming } => bar
            .when(confirming, |el| el.visible())
            .child(icon_button(("edit", ix), "✎", t).on_click(cx.listener(
                move |shell, _, window, cx| {
                    cx.stop_propagation();
                    let draft = shell.connection(ix);
                    shell.open_form(Some(ix), draft, window, cx);
                },
            )))
            .child(
                icon_button(("delete", ix), "×", t)
                    .when(confirming, |el| el.text_color(t.danger))
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        cx.stop_propagation();
                        shell.delete_connection(ix, cx);
                    })),
            ),
        RowKind::SshConfig => {
            let draft = crate::connections::Connection::from_entry(host);
            bar.child(icon_button(("duplicate", ix), "⧉", t).on_click(cx.listener(
                move |shell, _, window, cx| {
                    cx.stop_propagation();
                    shell.open_form(None, Some(draft.clone()), window, cx);
                },
            )))
        }
    }
}

fn row(
    ix: usize,
    host: &HostEntry,
    status: Option<&Status>,
    active: bool,
    kind: RowKind,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let target = host.clone();
    let confirming = matches!(kind, RowKind::Connection { confirming: true });
    let subtitle = if confirming {
        "Click × again to delete".to_owned()
    } else {
        address(host)
    };
    let hover_actions = actions(ix, host, kind, t, cx);
    div()
        .id(("host", ix))
        .group("host-row")
        .px(px(SPACE_SM))
        .py(px(6.))
        .rounded(px(CONTROL_RADIUS))
        .flex()
        .items_center()
        .gap(px(SPACE_SM))
        .cursor_pointer()
        .when(active, |el| el.bg(t.row_active))
        .hover(|s| s.bg(t.row_hover))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.connect_host(target.clone(), window, cx);
        }))
        .child(match status {
            Some(s) => tabs::status_dot(("host-dot", ix), s, t).into_any_element(),
            None => div().flex_none().size(px(6.)).into_any_element(),
        })
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(SharedString::from(host.alias.clone())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if confirming { t.danger } else { t.muted })
                        .truncate()
                        .child(SharedString::from(subtitle)),
                ),
        )
        .child(div().flex_1())
        .child(hover_actions)
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
    }
}
