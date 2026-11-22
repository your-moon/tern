// Adapted from zeron apps/zgx/ui/screens/settings/appearance.luau (font_picker)
// and apps/zgx/ui/ds/dropdown.luau (MIT).
use super::{ContextMenu, FontTarget, Shell, font_families};
use crate::icons::{self, icon};
use crate::text_input::{InputColors, TextInput};
use gpui::prelude::*;
use gpui::{
    AnyElement, Entity, Focusable, KeyDownEvent, Pixels, Point, ScrollHandle, Subscription, Window,
    anchored, deferred, div, px,
};

pub(in crate::shell) struct FontMenu {
    target: FontTarget,
    position: Point<Pixels>,
    search: Entity<TextInput>,
    _repaint: Subscription,
    scroll: ScrollHandle,
    active: usize,
    current: Option<String>,
}

// None selects the bundled default. Keep a saved family visible if another device lacks it.
fn options<'a>(menu: &'a FontMenu, query: &str) -> Vec<(Option<&'a str>, String)> {
    let (default, terminal) = match menu.target {
        FontTarget::Interface => (crate::theme::UI_FONT, false),
        FontTarget::Terminal => (crate::theme::MONO_FONT, true),
    };
    let mut rows = vec![(None, format!("Default ({default})"))];
    if let Some(installed) = font_families::cached() {
        rows.extend(
            installed
                .iter()
                .filter(|(_, mono)| !terminal || *mono)
                .map(|(name, _)| (Some(name.as_str()), name.clone())),
        );
    }
    if let Some(name) = menu.current.as_deref()
        && !rows.iter().any(|(value, _)| *value == Some(name))
    {
        rows.insert(1, (Some(name), name.to_owned()));
    }
    let terms = query.trim().to_lowercase();
    if !terms.is_empty() {
        rows.retain(|(_, label)| label.to_lowercase().contains(&terms));
    }
    rows
}

impl FontMenu {
    fn reveal_current(&mut self) {
        self.active = options(self, "")
            .iter()
            .position(|(family, _)| *family == self.current.as_deref())
            .unwrap_or(0);
        self.scroll.scroll_to_item(self.active);
    }
}

impl Shell {
    pub(in crate::shell) fn font_button(
        &self,
        target: FontTarget,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let (id, label, current, default) = match target {
            FontTarget::Interface => (
                "interface-family",
                "Interface font",
                &self.settings.interface_font_family,
                crate::theme::UI_FONT,
            ),
            FontTarget::Terminal => (
                "terminal-family",
                "Terminal font",
                &self.settings.terminal_font_family,
                crate::theme::MONO_FONT,
            ),
        };
        let t = self.theme;
        // Zeron anchors its dropdown to the trigger's bottom left with a six-point gap.
        let bounds = std::rc::Rc::new(std::cell::Cell::new(None::<gpui::Bounds<Pixels>>));
        let measured = bounds.clone();
        div()
            .on_children_prepainted(move |children, _, _| measured.set(children.first().copied()))
            .child(
                div()
                    .id(id)
                    .role(gpui::Role::Button)
                    .aria_label(label)
                    .tab_index(0)
                    .h(px(26.))
                    .max_w(px(240.))
                    .px(px(8.))
                    .gap(px(4.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(t.text)
                    .bg(t.ink(if t.light { 0.05 } else { 0.08 }))
                    .hover(|button| button.bg(t.row_hover))
                    .active(|button| button.bg(t.row_active))
                    .cursor_pointer()
                    .child(
                        div().min_w_0().truncate().child(
                            current
                                .clone()
                                .unwrap_or_else(|| format!("Default ({default})")),
                        ),
                    )
                    .child(
                        icon(icons::CHEVRON_RIGHT)
                            .size(px(12.))
                            .flex_none()
                            .text_color(t.muted)
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                0.25,
                            ))),
                    )
                    .on_click(cx.listener(move |s, e: &gpui::ClickEvent, window, cx| {
                        let point = bounds
                            .get()
                            .map(|b| b.bottom_left())
                            .unwrap_or_else(|| e.position());
                        s.open_font_menu(target, point + gpui::point(px(0.), px(6.)), window, cx);
                    })),
            )
    }

    fn open_font_menu(
        &mut self,
        target: FontTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = match target {
            FontTarget::Interface => self.settings.interface_font_family.clone(),
            FontTarget::Terminal => self.settings.terminal_font_family.clone(),
        };
        let colors = *cx.global::<InputColors>();
        let search = cx.new(|cx| TextInput::new("Search fonts…", false, colors, cx));
        window.focus(&search.focus_handle(cx), cx);
        let mut last_query = String::new();
        let repaint = cx.observe(&search, move |shell, input, cx| {
            if let Some(ContextMenu::Fonts(menu)) = &mut shell.context_menu {
                let query = input.read(cx).text();
                if last_query != query {
                    last_query = query.to_owned();
                    menu.active = 0;
                    menu.scroll.scroll_to_item(0);
                    cx.notify();
                }
            }
        });
        let mut menu = FontMenu {
            target,
            position,
            search,
            _repaint: repaint,
            scroll: ScrollHandle::new(),
            active: 0,
            current,
        };
        menu.reveal_current();
        self.context_menu = Some(ContextMenu::Fonts(menu));
        if font_families::cached().is_none() {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .spawn(async {
                        font_families::load();
                    })
                    .await;
                let _ = this.update(cx, |shell, cx| {
                    if let Some(ContextMenu::Fonts(menu)) = &mut shell.context_menu {
                        if menu.search.read(cx).text().is_empty() {
                            menu.reveal_current();
                        }
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn select_font(&mut self, value: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ContextMenu::Fonts(menu)) = self.context_menu.take() else {
            return;
        };
        match menu.target {
            FontTarget::Interface => self.update_settings(|s| s.interface_font_family = value, cx),
            FontTarget::Terminal => self.set_terminal_font(value, cx),
        }
        window.focus(&self.focus, cx);
    }

    fn font_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ContextMenu::Fonts(menu)) = &mut self.context_menu else {
            return;
        };
        let rows = options(menu, menu.search.read(cx).text());
        match event.keystroke.key.as_str() {
            "up" | "down" if !rows.is_empty() => {
                let delta = if event.keystroke.key == "up" { -1 } else { 1 };
                menu.active = crate::tabs::step(menu.active, rows.len(), delta);
                menu.scroll.scroll_to_item(menu.active);
                cx.notify();
            }
            "enter" => {
                if let Some((name, _)) = rows.get(menu.active) {
                    let value = name.map(str::to_owned);
                    self.select_font(value, window, cx);
                }
            }
            "escape" => {
                self.close_menu(cx);
                window.focus(&self.focus, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(super) fn render_font_menu(&self, menu: &FontMenu, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let found = options(menu, menu.search.read(cx).text());
        let mut list = div()
            .id("font-results")
            .max_h(px(220.))
            .overflow_y_scroll()
            .track_scroll(&menu.scroll)
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(2.));
        if found.is_empty() {
            list = list.child(div().p(px(12.)).text_color(t.muted).child("No fonts found"));
        }
        for (i, (value, label)) in found.into_iter().enumerate() {
            let chosen = menu.current.as_deref() == value;
            let value = value.map(str::to_owned);
            list = list.child(
                div()
                    .id(("font-choice", i))
                    .role(gpui::Role::Button)
                    .aria_label(label.clone())
                    .h(px(28.))
                    .flex_none()
                    .rounded(px(6.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .when(i == menu.active, |row| row.bg(t.row_active))
                    .hover(|row| row.bg(t.row_hover))
                    .on_click(cx.listener(move |s, _, w, cx| s.select_font(value.clone(), w, cx)))
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .when(chosen, |row| row.child("✓")),
            );
        }
        let card = div()
            .id("font-picker")
            .occlude()
            .w(px(280.))
            .rounded(px(12.))
            .border_1()
            .border_color(t.border)
            .bg(t.popup)
            .shadow_lg()
            .text_size(px(13.))
            .text_color(t.text)
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::font_key))
            .on_mouse_down_out(cx.listener(|s, _, w, cx| {
                s.close_menu(cx);
                w.focus(&s.focus, cx);
            }))
            .child(
                div()
                    .h(px(36.))
                    .flex_none()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.hairline)
                    .child(icon(icons::SEARCH).size(px(14.)).text_color(t.muted))
                    .child(div().min_w_0().flex_1().child(menu.search.clone())),
            )
            .child(list);
        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window_with_margin(px(8.))
                .child(card),
        )
        .priority(2)
        .into_any_element()
    }
}
