// Adapted from zeron crates/ui/src/popover.rs (popover_card, menu_row, menu_separator) (MIT).
//! The right-click menu for host rows and tabs: every action on a host in one place, opened
//! at the pointer. A click outside, Escape or choosing an item closes it.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Point,
    SharedString, StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px,
};
use tern_ssh::{ConnectSpec, HostEntry};

use super::{Shell, ThemeTarget};
use crate::icons::{self, icon};

type Run = Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>;

enum Item {
    Action {
        icon: &'static str,
        label: &'static str,
        destructive: bool,
        run: Run,
    },
    Separator,
}

fn action(
    icon: &'static str,
    label: &'static str,
    run: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
) -> Item {
    Item::Action {
        icon,
        label,
        destructive: false,
        run: Rc::new(run),
    }
}

fn destructive(
    icon: &'static str,
    label: &'static str,
    run: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
) -> Item {
    Item::Action {
        icon,
        label,
        destructive: true,
        run: Rc::new(run),
    }
}

pub(super) struct ContextMenu {
    position: Point<Pixels>,
    items: Vec<Item>,
}

impl Shell {
    /// Right-click on a host row; `editable` is its index among tern's connections.
    pub(crate) fn open_host_menu(
        &mut self,
        host: &HostEntry,
        editable: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let alias = host.alias.clone();
        let connect = host.clone();
        let mut items = vec![action(icons::TERMINAL, "Connect", move |s, w, cx| {
            s.connect_host(connect.clone(), w, cx)
        })];
        let fresh = host.clone();
        items.push(action(icons::PLUS, "Open in new tab", move |s, w, cx| {
            s.open_new_tab(&fresh, w, cx)
        }));
        items.push(Item::Separator);
        items.push(action(icons::PEN, "Edit…", move |s, w, cx| {
            let draft = s.connection(editable);
            s.open_form(Some(editable), draft, w, cx);
        }));
        let theme_alias = alias.clone();
        items.push(action(icons::PALETTE, "Theme…", move |s, w, cx| {
            s.open_theme_picker(ThemeTarget::Host(theme_alias.clone()), w, cx)
        }));
        items.push(Item::Separator);
        // The menu is already a deliberate second step, so remove at once; Undo covers it.
        items.push(destructive(icons::TRASH, "Remove", move |s, _, cx| {
            s.confirm_delete = Some(editable);
            s.delete_connection(editable, cx);
        }));
        self.context_menu = Some(ContextMenu { position, items });
        cx.notify();
    }

    /// Right-click on a tab.
    pub(crate) fn open_tab_menu(
        &mut self,
        ix: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        let alias = tab.alias.clone();
        let closed = tab.session.read(cx).status.is_dormant();
        let local = tab.session.read(cx).is_local();
        let logging = tab.session.read(cx).is_logging();
        let broadcasting = self.is_broadcasting(tab.session.entity_id());
        let mut items = Vec::new();
        if closed {
            items.push(action(icons::RESTART, "Reconnect", move |s, w, cx| {
                if let Some(tab) = s.tabs.get(ix) {
                    let session = tab.session.clone();
                    session.update(cx, |s, cx| s.reconnect(cx));
                }
                s.activate_tab(ix, w, cx);
            }));
        }
        let again = alias.clone();
        if local {
            items.push(action(icons::PLUS, "New local terminal", |s, w, cx| {
                s.open_local_tab(w, cx)
            }));
        } else {
            items.push(action(
                icons::PLUS,
                "New tab to this host",
                move |s, w, cx| match s.hosts.iter().find(|h| h.alias == again).cloned() {
                    Some(host) => s.open_new_tab(&host, w, cx),
                    None => s.connect_target(&again, w, cx),
                },
            ));
        }
        items.push(action(icons::PEN, "Rename…", move |s, w, cx| {
            s.start_rename(ix, w, cx)
        }));
        if broadcasting {
            items.push(action(icons::CLOSE, "Stop broadcasting", |s, _, cx| {
                s.stop_broadcast(cx)
            }));
        }
        items.push(action(
            icons::KEYBOARD,
            "Broadcast input to…",
            move |s, _, cx| s.open_broadcast_picker(ix, position, cx),
        ));
        if logging {
            items.push(action(icons::INFO, "Stop logging", move |s, _, cx| {
                s.stop_logging(ix, cx)
            }));
        } else {
            items.push(action(icons::INFO, "Start logging…", move |s, _, cx| {
                s.start_logging(ix, cx)
            }));
        }
        let theme_alias = alias.clone();
        items.push(action(icons::PALETTE, "Theme…", move |s, w, cx| {
            s.open_theme_picker(ThemeTarget::Host(theme_alias.clone()), w, cx)
        }));
        items.push(Item::Separator);
        items.push(destructive(icons::CLOSE, "Close tab", move |s, w, cx| {
            s.close_tab_at(ix, w, cx)
        }));
        if self.tabs.len() > 1 {
            items.push(destructive(
                icons::CLOSE,
                "Close other tabs",
                move |s, w, cx| s.close_other_tabs(ix, w, cx),
            ));
        }
        self.context_menu = Some(ContextMenu { position, items });
        cx.notify();
    }

    /// A second session to the same host, even when one is already open.
    fn open_new_tab(&mut self, host: &HostEntry, window: &mut Window, cx: &mut Context<Self>) {
        match ConnectSpec::from_host_entry(host) {
            Ok(spec) => self.open_tab(super::Launch::Ssh(spec), host.alias.clone(), window, cx),
            Err(e) => self.notify_toast(super::ToastKind::Critical, e.to_string(), cx),
        }
    }

    fn close_other_tabs(&mut self, keep: usize, window: &mut Window, cx: &mut Context<Self>) {
        if keep >= self.tabs.len() {
            return;
        }
        let kept = self.tabs.remove(keep);
        self.tabs = vec![kept];
        self.sync_broadcast(cx);
        self.activate_tab(0, window, cx);
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let t = self.theme;
        let mut card = div()
            .id("context-menu")
            .occlude()
            .w(px(216.))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded(px(12.))
            .border_1()
            .border_color(t.border)
            .bg(t.popup)
            .shadow_lg()
            .text_size(px(13.))
            .text_color(t.text)
            .on_mouse_down_out(cx.listener(|s, _, _, cx| s.close_menu(cx)));
        for (ix, item) in menu.items.iter().enumerate() {
            card = match item {
                Item::Separator => {
                    card.child(div().h(px(1.)).mx(px(-4.)).my(px(2.)).bg(t.hairline))
                }
                Item::Action {
                    icon: glyph,
                    label,
                    destructive,
                    run,
                } => {
                    let run = run.clone();
                    card.child(
                        div()
                            .id(("menu-item", ix))
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(7.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .cursor_pointer()
                            .text_color(t.text.opacity(0.9))
                            .when(*destructive, |el| el.text_color(t.danger))
                            .hover(|s| s.bg(t.row_active))
                            .on_click(cx.listener(move |s, _, w, cx| {
                                s.context_menu = None;
                                run(s, w, cx);
                                cx.notify();
                            }))
                            .child(icon(glyph).size(px(16.)).text_color(if *destructive {
                                t.danger
                            } else {
                                t.muted
                            }))
                            .child(SharedString::from(*label)),
                    )
                }
            };
        }
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(card),
            )
            .priority(2)
            .into_any_element(),
        )
    }
}
