// Adapted from zeron crates/ui/src/shell.rs (JumpSession slot action, titlebar group rhythm) (MIT).
//! Session tabs in the titlebar strip and the shortcuts that move between them.

use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    Action, Animation, AnimationExt, AnyElement, AppContext, Context, ElementId, Entity,
    FontWeight, Hsla, InteractiveElement, IntoElement, KeyBinding, MouseButton, ParentElement,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, actions, div,
    pulsating_between, px,
};

use crate::session::Status;
use crate::shell::Shell;
use crate::text_input::TextInput;
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
    /// The name the user gave the tab; it shows instead of the alias.
    pub title: Option<String>,
    /// A shell on this machine rather than a connection.
    pub local: bool,
    pub status: Status,
    /// The field that is editing this tab's title right now.
    pub rename: Option<Entity<TextInput>>,
}

impl TabInfo {
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.alias)
    }
}

/// What a tab drag carries: the slot it started from and what to draw under the pointer.
#[derive(Clone)]
pub struct DraggedTab {
    pub ix: usize,
    label: SharedString,
    background: Hsla,
    color: Hsla,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(26.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(CONTROL_RADIUS))
            .bg(self.background)
            .text_color(self.color)
            .text_sm()
            .shadow_md()
            .child(self.label.clone())
    }
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
    let dragged = DraggedTab {
        ix,
        label: SharedString::from(middle_ellipsis(tab.label(), 18)),
        background: t.popup,
        color: t.text,
    };
    let accent = t.accent;
    let renaming = tab.rename.clone();
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
        .on_click(cx.listener(move |shell, e: &gpui::ClickEvent, window, cx| {
            // A click inside the title field is for the field, not for the tab.
            if shell.is_renaming(ix) {
                return;
            }
            if e.click_count() >= 2 {
                shell.start_rename(ix, window, cx);
            } else {
                shell.activate_tab(ix, window, cx);
            }
        }))
        .on_drag(dragged, |tab, _, _, cx| cx.new(|_| tab.clone()))
        .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(accent.opacity(0.25)))
        .on_drop(cx.listener(move |shell, from: &DraggedTab, window, cx| {
            shell.move_tab(from.ix, ix, window, cx)
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |shell, e: &gpui::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                shell.open_tab_menu(ix, e.position, cx);
            }),
        )
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener(move |shell, _, window, cx| shell.close_tab_at(ix, window, cx)),
        )
        .child(if tab.local {
            local_glyph(&tab.status, t)
        } else {
            status_dot(("tab-dot", ix), &tab.status, t)
        })
        .child(match renaming {
            Some(input) => div()
                .flex_1()
                .min_w_0()
                .child(input)
                .on_mouse_down_out(cx.listener(|shell, _, window, cx| {
                    shell.commit_rename(window, cx);
                }))
                .into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(active, |el| el.font_weight(FontWeight::MEDIUM))
                .child(SharedString::from(middle_ellipsis(tab.label(), 18)))
                .into_any_element(),
        })
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

/// The mark of a local shell tab: a terminal glyph where a connection has its status dot,
/// red once the shell has exited.
fn local_glyph(status: &Status, t: &Theme) -> AnyElement {
    let color = if *status == Status::Closed {
        t.danger
    } else {
        t.muted
    };
    crate::icons::icon(crate::icons::TERMINAL)
        .size(px(14.))
        .text_color(color)
        .into_any_element()
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

/// Moves the tab at `from` so it ends up at `to`, and returns where the active tab is now, so
/// the one in front stays in front whichever tab was dragged.
pub fn reorder<T>(items: &mut Vec<T>, active: usize, from: usize, to: usize) -> usize {
    if from >= items.len() || to >= items.len() || from == to {
        return active;
    }
    let item = items.remove(from);
    items.insert(to, item);
    if active == from {
        to
    } else if from < active && to >= active {
        active - 1
    } else if from > active && to <= active {
        active + 1
    } else {
        active
    }
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
    use super::{reorder, step};

    /// Tabs named a..e, so a wrong move shows in the order and not only in an index.
    fn tabs() -> Vec<&'static str> {
        vec!["a", "b", "c", "d", "e"]
    }

    #[test]
    fn dragging_right_puts_the_tab_at_the_drop_slot() {
        let mut t = tabs();
        let active = reorder(&mut t, 4, 1, 3);
        assert_eq!(t, ["a", "c", "d", "b", "e"]);
        assert_eq!(active, 4, "e was active and did not move");
    }

    #[test]
    fn dragging_left_puts_the_tab_at_the_drop_slot() {
        let mut t = tabs();
        let active = reorder(&mut t, 0, 3, 1);
        assert_eq!(t, ["a", "d", "b", "c", "e"]);
        assert_eq!(active, 0);
    }

    #[test]
    fn first_and_last_slots() {
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 2, 4, 0), 3);
        assert_eq!(t, ["e", "a", "b", "c", "d"]);
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 2, 0, 4), 1);
        assert_eq!(t, ["b", "c", "d", "e", "a"]);
    }

    #[test]
    fn the_active_tab_follows_its_own_drag() {
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 1, 1, 3), 3);
        assert_eq!(t[3], "b");
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 3, 3, 0), 0);
        assert_eq!(t[0], "d");
    }

    #[test]
    fn another_tab_passing_the_active_one_shifts_it() {
        // c is active; a is dragged past it to the right.
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 2, 0, 3), 1);
        assert_eq!(t[1], "c");
        // c is active; e is dragged past it to the left.
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 2, 4, 1), 3);
        assert_eq!(t[3], "c");
        // Dropping exactly onto the active slot still passes it.
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 2, 0, 2), 1);
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 2, 4, 2), 3);
    }

    #[test]
    fn a_move_that_does_not_cross_the_active_tab_leaves_it() {
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 4, 0, 2), 4);
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 0, 4, 2), 0);
    }

    #[test]
    fn no_op_and_out_of_range_moves_change_nothing() {
        let mut t = tabs();
        assert_eq!(reorder(&mut t, 2, 3, 3), 2);
        assert_eq!(reorder(&mut t, 2, 9, 0), 2);
        assert_eq!(reorder(&mut t, 2, 0, 9), 2);
        assert_eq!(t, tabs());
    }

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
