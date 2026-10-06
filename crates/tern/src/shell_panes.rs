//! Splits: a tab holds one or more panes laid out as a tree (see `split.rs`). A split opens
//! another session like the focused one; ⌘W closes a pane before it closes the tab.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Focusable, InteractiveElement, IntoElement, ParentElement, Styled, Window,
    div, px, transparent_black,
};

use super::{Shell, Tab};
use crate::split::{Axis, Direction, Node, PaneId};

impl Shell {
    /// Splits the focused pane of the front tab; the new pane has the keyboard.
    pub(crate) fn split_pane(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let launch = tab.session().read(cx).launch_again();
        let alias = tab.alias.clone();
        let (target, ix) = (tab.focused, self.active);
        let pane = self.make_pane(launch, &alias, window, cx);
        let id = pane.id;
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        if !tab.tree.split(target, axis, id) {
            return;
        }
        tab.panes.push(pane);
        tab.focused = id;
        self.focus_front_pane(window, cx);
    }

    /// ⌘W: a split tab loses the focused pane, a tab with one pane closes.
    pub(crate) fn close_pane_or_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if tab.panes.len() > 1 {
            self.close_pane(window, cx);
        } else {
            self.close_tab_at(self.active, window, cx);
        }
    }

    /// Closes the focused pane (its session ends); the pane before it, or the one after for
    /// the first, takes the keyboard.
    fn close_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let order = tab.tree.leaves();
        let Some(at) = order.iter().position(|id| *id == tab.focused) else {
            return;
        };
        let next = if at > 0 { order[at - 1] } else { order[1] };
        let tree = std::mem::replace(&mut tab.tree, Node::Leaf(tab.focused));
        let Some(rest) = tree.remove(tab.focused) else {
            return;
        };
        tab.tree = rest;
        tab.panes.retain(|p| p.id != tab.focused);
        tab.focused = next;
        self.focus_front_pane(window, cx);
    }

    pub(crate) fn focus_pane(&mut self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if tab.focused == id || !tab.tree.contains(id) {
            return;
        }
        tab.focused = id;
        self.focus_front_pane(window, cx);
    }

    pub(crate) fn move_pane_focus(
        &mut self,
        dir: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if let Some(to) = tab.tree.neighbour(tab.focused, dir) {
            self.focus_pane(to, window, cx);
        }
    }

    /// Gives the keyboard to the focused pane of the front tab, and points a broadcast at it.
    fn focus_front_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            let focus = tab.session().read(cx).view.focus_handle(cx);
            window.focus(&focus, cx);
        }
        self.apply_broadcast(cx);
        cx.notify();
    }

    /// The tab's terminals, in their split layout.
    pub(super) fn render_panes(&self, tab: &Tab, cx: &mut Context<Self>) -> AnyElement {
        self.render_node(tab, &tab.tree, tab.panes.len() > 1, cx)
    }

    fn render_node(
        &self,
        tab: &Tab,
        node: &Node,
        split: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        match node {
            Node::Leaf(id) => {
                let Some(pane) = tab.panes.iter().find(|p| p.id == *id) else {
                    return div().into_any_element();
                };
                let id = *id;
                let focused = tab.focused == id;
                div()
                    .id(("pane", id))
                    .size_full()
                    // Only a split tab marks its focus; a lone pane would just wear a frame.
                    .when(split, |el| {
                        el.border_1().border_color(if focused {
                            t.accent.opacity(0.55)
                        } else {
                            transparent_black()
                        })
                    })
                    // Before the terminal sees the click, so selecting text in a pane also
                    // gives it the keyboard.
                    .capture_any_mouse_down(cx.listener(move |s, _, w, cx| s.focus_pane(id, w, cx)))
                    .child(pane.session.read(cx).view.clone())
                    .into_any_element()
            }
            Node::Split {
                axis,
                first,
                second,
            } => {
                let half = |el: gpui::Div| el.flex_1().min_w(px(0.)).min_h(px(0.));
                let a = self.render_node(tab, first, split, cx);
                let b = self.render_node(tab, second, split, cx);
                let divider = match axis {
                    Axis::Row => div().flex_none().bg(t.border).w(px(1.)).h_full(),
                    Axis::Column => div().flex_none().bg(t.border).h(px(1.)).w_full(),
                };
                div()
                    .size_full()
                    .flex()
                    .when(*axis == Axis::Column, |el| el.flex_col())
                    .child(half(div()).child(a))
                    .child(divider)
                    .child(half(div()).child(b))
                    .into_any_element()
            }
        }
    }
}
