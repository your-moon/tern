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

/// `open` are the tabs; a host shows the status of its newest tab and is highlighted when
/// that tab is the active one.
pub fn render(
    hosts: &[HostEntry],
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
        rows.push(row(ix, host, status, active, t, cx));
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
                .text_xs()
                .text_color(t.faint)
                .child("Hosts"),
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
                            .child("No hosts in ~/.ssh/config"),
                    )
                }),
        )
}

fn row(
    ix: usize,
    host: &HostEntry,
    status: Option<&Status>,
    active: bool,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let target = host.clone();
    div()
        .id(("host", ix))
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
                        .text_color(t.muted)
                        .truncate()
                        .child(SharedString::from(address(host))),
                ),
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
