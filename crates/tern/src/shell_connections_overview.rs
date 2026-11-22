//! The Connections page uses the same Zeron page column, cards and rows as Settings.
use super::*;
use crate::settings_widgets as widgets;
use std::collections::{HashMap, HashSet};

impl Shell {
    pub(crate) fn show_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_settings(window, cx);
        self.intro_entered = true;
        self.error = None;
        self.renaming = None;
        self.active = self.tabs.len();
        self.workspaces.empty = None;
        self.persist_tabs();
        self.restore_focus(window, cx);
    }

    pub(super) fn connections_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = &self.theme;
        let mut page = widgets::page_column().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(widgets::page_header(t, "Connections"))
                .child(
                    widgets::action_button(t, "overview-new", "New connection", true)
                        .on_click(cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx))),
                ),
        );
        let mut sessions: HashMap<&str, Vec<usize>> = HashMap::new();
        for (ix, tab) in self.tabs.iter().enumerate() {
            sessions.entry(&tab.alias).or_default().push(ix);
        }
        let mut seen = HashSet::new();
        for (n, (alias, address)) in self
            .hosts
            .iter()
            .map(|h| (h.alias.as_str(), Some(sidebar::address(h))))
            .chain(self.tabs.iter().map(|tab| (tab.alias.as_str(), None)))
            .filter(|(alias, _)| seen.insert(*alias))
            .enumerate()
        {
            let open_alias = alias.to_owned();
            let arrow = || {
                crate::icons::icon(crate::icons::CHEVRON_RIGHT)
                    .size(px(14.))
                    .text_color(t.muted)
            };
            let mut group = widgets::card(t).mt(px(16.)).flex_none().child(
                widgets::row(t, true, alias.to_owned(), address.map(Into::into), arrow())
                    .id(("overview-connection", n))
                    .cursor_pointer()
                    .hover(|el| el.bg(t.row_hover))
                    .on_click(
                        cx.listener(move |s, _, w, cx| s.select_connection(&open_alias, w, cx)),
                    ),
            );
            for (slot, &ix) in sessions.get(alias).into_iter().flatten().enumerate() {
                let tab = &self.tabs[ix];
                let title = tab
                    .title
                    .clone()
                    .unwrap_or_else(|| format!("Terminal {}", slot + 1));
                group = group.child(
                    widgets::row(t, false, title, None, arrow())
                        .id(("overview-session", ix))
                        .pl(px(32.))
                        .cursor_pointer()
                        .hover(|el| el.bg(t.row_hover))
                        .on_click(cx.listener(move |s, _, w, cx| s.show_tab(ix, w, cx))),
                );
            }
            page = page.child(group);
        }
        if seen.is_empty() {
            page = page.child(widgets::page_subtitle(t, "No connections yet."));
        }
        div()
            .id("connections-page")
            .size_full()
            .overflow_y_scroll()
            .child(page)
            .into_any_element()
    }
}
