// Adapted from zeron crates/ui/src/shell.rs (JumpSession slot action, titlebar group rhythm) (MIT).
//! Session tabs in the titlebar strip and the shortcuts that move between them.

use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    Action, Animation, AnimationExt, AnyElement, Context, ElementId, FontWeight,
    InteractiveElement, IntoElement, KeyBinding, MouseButton, ParentElement, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, actions, div, pulsating_between, px,
};

use crate::session::Status;
use crate::shell::Shell;
use crate::theme::{CONTROL_RADIUS, Theme};

actions!(tern, [CloseTab, NextTab, PrevTab]);

/// Activate the tab at a zero-based slot: one action carrying the slot, as zeron's
/// `JumpSession`, rather than nine near-identical actions.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = tern, no_json)]
pub struct ActivateTab(pub usize);

pub fn bindings() -> Vec<KeyBinding> {
    // ⌘W and ⌘⇧[ / ⌘⇧] are in the keymap; these second chords and the slots stay fixed.
    let mut b = vec![
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
    ];
    for slot in 0..9 {
        b.push(KeyBinding::new(
            &format!("cmd-{}", slot + 1),
            ActivateTab(slot),
            None,
        ));
    }
    b
}

/// What the strip needs to know about one tab.
pub struct TabInfo {
    pub alias: String,
    pub status: Status,
}

/// Tabs shrink like a browser's (200 wide down to 72) and the strip scrolls sideways once
/// they no longer fit; `scroll` keeps the active one in view.
pub fn strip(
    tabs: &[TabInfo],
    active: usize,
    scroll: &ScrollHandle,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let mut row = div()
        .id("tab-strip")
        .flex()
        .items_center()
        .gap(px(2.))
        .min_w_0()
        .flex_shrink(1.)
        .overflow_x_scroll()
        .track_scroll(scroll);
    for (ix, tab) in tabs.iter().enumerate() {
        row = row.child(tab_pill(ix, tab, ix == active, t, cx));
    }
    row
}

fn tab_pill(
    ix: usize,
    tab: &TabInfo,
    active: bool,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    div()
        .id(("tab", ix))
        .group("tab")
        .h(px(26.))
        .flex_shrink(1.)
        .min_w(px(72.))
        .max_w(px(200.))
        .pl(px(10.))
        .pr(px(4.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(CONTROL_RADIUS))
        .cursor_pointer()
        .text_sm()
        .when(active, |el| el.bg(t.row_active).text_color(t.text))
        .when(!active, |el| {
            el.text_color(t.muted).hover(|s| s.bg(t.row_hover))
        })
        // A click on a tab must not also start a window drag from the titlebar.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |shell, _, window, cx| shell.activate_tab(ix, window, cx)))
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener(move |shell, _, window, cx| shell.close_tab_at(ix, window, cx)),
        )
        .child(status_dot(("tab-dot", ix), &tab.status, t))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(active, |el| el.font_weight(FontWeight::MEDIUM))
                .child(SharedString::from(middle_ellipsis(&tab.alias, 18))),
        )
        .child(
            div()
                .id(("close", ix))
                .size(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .text_color(t.faint)
                .invisible()
                .group_hover("tab", |s| s.visible())
                .when(active, |el| el.visible())
                .hover(|s| s.bg(t.row_hover).text_color(t.text))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |shell, _, window, cx| {
                    cx.stop_propagation();
                    shell.close_tab_at(ix, window, cx);
                }))
                .child("×"),
        )
}

/// zeron's `zeron-pulse` loader: 2.4 s, opacity 0.08 → 1.
const PULSE: Duration = Duration::from_millis(2400);

/// A connecting dot breathes; gpui holds it still when `App::reduce_motion` is set. Synced, so
/// a host's tab dot and sidebar dot pulse together.
pub fn status_dot(id: impl Into<ElementId>, status: &Status, t: &Theme) -> AnyElement {
    let color = match status {
        Status::Connected => t.success,
        Status::Connecting => t.accent,
        Status::Closed => t.danger,
    };
    let dot = div().flex_none().size(px(6.)).rounded_full().bg(color);
    if *status == Status::Connecting {
        dot.with_animation(
            id,
            Animation::new(PULSE)
                .repeat_synced()
                .with_easing(pulsating_between(0.08, 1.0)),
            |dot, alpha| dot.opacity(alpha),
        )
        .into_any_element()
    } else {
        dot.into_any_element()
    }
}

/// Long names lose their middle, not their end: hosts named alike ("db-replica-01",
/// "db-replica-02") usually differ at the end, which plain truncation would cut.
pub fn middle_ellipsis(name: &str, max_chars: usize) -> String {
    let count = name.chars().count();
    if count <= max_chars || max_chars < 5 {
        return name.to_owned();
    }
    // A third for the head, the rest for the tail, where names like these differ.
    let keep = max_chars - 1;
    let head = keep / 3;
    let tail = keep - head;
    let start: String = name.chars().take(head).collect();
    let end: String = name.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

/// The slot after `active` when moving by `delta`, wrapping at both ends.
pub fn step(active: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (active as isize + delta).rem_euclid(len as isize) as usize
}

#[cfg(test)]
mod tests {
    use super::step;

    #[test]
    fn long_names_keep_their_distinct_end() {
        use super::middle_ellipsis;
        assert_eq!(middle_ellipsis("short", 18), "short");
        let a = middle_ellipsis("production-database-replica-01", 18);
        let b = middle_ellipsis("production-database-replica-02", 18);
        assert_ne!(a, b);
        assert_eq!(a.chars().count(), 18);
        assert!(a.ends_with("replica-01"), "{a}");
    }

    #[test]
    fn stepping_wraps_both_ways() {
        assert_eq!(step(0, 3, 1), 1);
        assert_eq!(step(2, 3, 1), 0);
        assert_eq!(step(0, 3, -1), 2);
        assert_eq!(step(0, 0, 1), 0);
    }
}
