// Adapted from Zeron apps/zgx/ui/ds/session_lanes.luau (MIT).
//! The connection directory groups terminal rows inside their host's card.
use super::*;
use crate::icons::{self, icon};
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

    pub(super) fn connections_overview(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = &self.theme;
        let wide = f32::from(window.viewport_size().width) - self.sidebar_now() >= 820.;
        let mut page = widgets::page_column()
            .max_w(px(1080.))
            .pt(px(32.))
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(26.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(t.text)
                            .child("Connections"),
                    )
                    .child(
                        widgets::action_button(t, "overview-new", "New connection", true)
                            .on_click(cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx))),
                    ),
            );
        let make_row = || div().flex().w_full().items_start().gap(px(16.));
        let mut row = make_row();
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
            let name = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(t.text)
                        .child(alias.to_owned()),
                )
                .when_some(address, |el, addr| {
                    el.child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(t.muted)
                            .child(addr),
                    )
                });
            let mut group = widgets::card(t).flex_1().min_w_0().child(
                div()
                    .id(("overview-connection", n))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .p(px(16.))
                    .cursor_pointer()
                    .hover(|el| el.bg(t.row_hover))
                    .on_click(
                        cx.listener(move |s, _, w, cx| s.select_connection(&open_alias, w, cx)),
                    )
                    .child(
                        div()
                            .size(px(38.))
                            .flex_none()
                            .rounded(px(10.))
                            .bg(t.field_fill())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(icons::SERVER).size(px(21.)).text_color(t.text)),
                    )
                    .child(name)
                    .child(icon(icons::CHEVRON_RIGHT).size(px(14.)).text_color(t.muted)),
            );
            let mut lanes = div()
                .border_t_1()
                .border_color(t.hairline)
                .p(px(4.))
                .flex()
                .flex_col();
            for (slot, &ix) in sessions.get(alias).into_iter().flatten().enumerate() {
                let tab = &self.tabs[ix];
                let title = tab
                    .title
                    .clone()
                    .unwrap_or_else(|| format!("Terminal {}", slot + 1));
                let session = tab.session().read(cx);
                let status = Dot::of(&session.status, session.is_asking());
                let label = if status == Dot::Idle {
                    "Not connected"
                } else {
                    status.label()
                };
                lanes = lanes.child(
                    div()
                        .id(("overview-session", ix))
                        .h(px(32.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .hover(|el| el.bg(t.row_hover))
                        .on_click(cx.listener(move |s, _, w, cx| s.show_tab(ix, w, cx)))
                        .child(icon(icons::TERMINAL).size(px(15.)).text_color(t.muted))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(px(13.))
                                .text_color(t.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(3.))
                                .text_size(px(11.))
                                .text_color(t.muted)
                                .child(sidebar::status_dot(Some(status), t))
                                .child(label),
                        ),
                );
            }
            if sessions.contains_key(alias) {
                group = group.child(lanes);
            }
            row = row.child(group);
            if !wide || n % 2 == 1 {
                page = page.child(row);
                row = make_row();
            }
        }
        if wide && seen.len() % 2 == 1 {
            page = page.child(row.child(div().flex_1().min_w_0()));
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
