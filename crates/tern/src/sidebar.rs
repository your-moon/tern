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
use crate::theme::{CONTROL_RADIUS, SIDEBAR_WIDTH, SPACE_SM, Theme};

pub fn render(
    hosts: &[HostEntry],
    active: Option<(&str, &Status)>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let mut rows = Vec::with_capacity(hosts.len());
    for (ix, host) in hosts.iter().enumerate() {
        let status = active.and_then(|(alias, s)| (alias == host.alias).then_some(s));
        rows.push(row(ix, host, status, t, cx));
    }
    div()
        .w(px(SIDEBAR_WIDTH))
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
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let target = host.clone();
    let dot = match status {
        Some(Status::Connected) => t.success,
        Some(Status::Connecting) => t.accent,
        Some(Status::Closed) => t.danger,
        None => t.faint.opacity(0.0),
    };
    div()
        .id(("host", ix))
        .px(px(SPACE_SM))
        .py(px(6.))
        .rounded(px(CONTROL_RADIUS))
        .flex()
        .items_center()
        .gap(px(SPACE_SM))
        .cursor_pointer()
        .when(status.is_some(), |el| el.bg(t.row_active))
        .hover(|s| s.bg(t.row_hover))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.connect_host(target.clone(), window, cx);
        }))
        .child(div().size(px(6.)).rounded_full().bg(dot))
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
fn address(host: &HostEntry) -> String {
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
