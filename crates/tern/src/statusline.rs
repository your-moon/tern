// Sizes from zeron crates/ui/src/composer.rs: SESSION_FOOTER_HEIGHT 24 (line 89), the footer
// row's 10 px side padding (line ~11474) and its 11 px text (lines 558, 4725) (MIT).
//! The strip under the terminal panel: which host the active tab is on, whether the
//! connection is up, and how long it has been. Round-trip latency is not shown: tern-ssh
//! sends keep-alives (russh) but exposes no timing for them.

use std::time::Duration;

use gpui::{Div, Hsla, IntoElement, ParentElement, Styled, div, px};

use crate::session::Status;
use crate::theme::Theme;

/// zeron `SESSION_FOOTER_HEIGHT`.
pub const HEIGHT: f32 = 24.0;
const SIDE_PAD: f32 = 10.0;
const TEXT_SIZE: f32 = 11.0;

/// `user@host:port`.
pub fn host_label(user: &str, host: &str, port: u16) -> String {
    format!("{user}@{host}:{port}")
}

/// `mm:ss` under an hour, `h:mm` from an hour on.
pub fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs >= 3600 {
        format!("{}:{:02}", secs / 3600, secs % 3600 / 60)
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}

fn state_label(status: &Status) -> &'static str {
    match status {
        Status::Connecting => "Connecting…",
        Status::Connected => "Connected",
        Status::Closed => "Disconnected",
        Status::Idle => "Not connected · Enter connects",
    }
}

/// The strip for one tab; `elapsed` is `None` until the session has connected.
pub fn render(
    t: &Theme,
    label: &str,
    status: &Status,
    elapsed: Option<Duration>,
    bg: Hsla,
) -> impl IntoElement + use<> {
    let dot = match status {
        Status::Connected => t.success,
        Status::Connecting => t.accent,
        Status::Closed => t.danger,
        Status::Idle => t.muted,
    };
    let part = |text: String| div().flex_none().child(text);
    let mut row: Div = div()
        .flex_none()
        .h(px(HEIGHT))
        .px(px(SIDE_PAD))
        .flex()
        .items_center()
        .gap(px(12.))
        .bg(bg)
        .border_t_1()
        .border_color(t.hairline)
        .text_size(px(TEXT_SIZE))
        .text_color(t.muted)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(t.text)
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().size(px(6.)).rounded_full().bg(dot))
                .child(state_label(status)),
        );
    if let Some(elapsed) = elapsed {
        row = row.child(part(format_elapsed(elapsed)));
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_is_minutes_and_seconds_then_hours_and_minutes() {
        let s = Duration::from_secs;
        assert_eq!(format_elapsed(s(0)), "00:00");
        assert_eq!(format_elapsed(s(59)), "00:59");
        assert_eq!(format_elapsed(s(61)), "01:01");
        assert_eq!(format_elapsed(s(3599)), "59:59");
        assert_eq!(format_elapsed(s(3600)), "1:00");
        assert_eq!(format_elapsed(s(3600 + 7 * 60 + 59)), "1:07");
        assert_eq!(format_elapsed(s(26 * 3600 + 5 * 60)), "26:05");
    }

    #[test]
    fn the_host_label_is_user_at_host_and_port() {
        assert_eq!(host_label("root", "10.0.0.5", 2222), "root@10.0.0.5:2222");
        assert_eq!(host_label("me", "example.org", 22), "me@example.org:22");
    }

    #[test]
    fn each_state_has_its_own_words() {
        assert_eq!(state_label(&Status::Connecting), "Connecting…");
        assert_eq!(state_label(&Status::Connected), "Connected");
        assert_eq!(state_label(&Status::Closed), "Disconnected");
    }
}
