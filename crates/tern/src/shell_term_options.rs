//! Settings → Terminal, and carrying those settings to every open terminal: cursor, scrollback,
//! mouse habits and the bell. Rows follow the zeron settings widgets the other pages use.

use crate::a11y::Accessible as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled,
    Subscription, Window, div, px,
};
use tern_term::{CursorStyleSetting, TerminalEvent, TerminalOptions};

use super::Shell;
use crate::session::Session;
use crate::settings::{CursorStyle, Settings};
use crate::settings_widgets as w;

impl Shell {
    pub(super) fn terminal_options(&self) -> TerminalOptions {
        let s = &self.settings;
        TerminalOptions {
            cursor_style: match s.cursor_style {
                CursorStyle::Block => CursorStyleSetting::Block,
                CursorStyle::Bar => CursorStyleSetting::Bar,
                CursorStyle::Underline => CursorStyleSetting::Underline,
            },
            cursor_blink: s.cursor_blink,
            scrollback_lines: s.scrollback_lines,
            copy_on_select: s.copy_on_select,
            middle_click_paste: s.middle_click_paste,
            visual_bell: s.visual_bell,
        }
    }

    pub(super) fn set_terminal_font(&mut self, family: Option<String>, cx: &mut Context<Self>) {
        self.update_settings(|st| st.terminal_font_family = family, cx);
        self.restyle_tabs(cx);
    }

    fn change_terminal(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        self.update_settings(change, cx);
        let options = self.terminal_options();
        for tab in &self.tabs {
            for pane in &tab.panes {
                let view = pane.session.read(cx).view.clone();
                view.update(cx, |v, cx| v.set_options(options.clone(), cx));
            }
        }
    }

    /// BEL from a session bounces the Dock icon while tern is behind other apps (macOS only
    /// bounces for an inactive app, so a bell in the window being used stays quiet).
    pub(super) fn on_bell(
        &self,
        session: &Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let terminal = session.read(cx).terminal();
        cx.subscribe_in(&terminal, window, |shell, _, event, window, _| {
            if matches!(event, TerminalEvent::Bell) && shell.settings.bell_bounces_dock {
                window.request_attention();
            }
        })
    }

    pub(super) fn terminal_page(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let s = &self.settings;
        let toggle = |id: &'static str, label: &'static str, on: bool, flip: fn(&mut Settings)| {
            div()
                .id(id)
                .switch(label, on)
                .cursor_pointer()
                .on_click(cx.listener(move |shell, _, _, cx| shell.change_terminal(flip, cx)))
                .child(w::toggle(&t, on, id))
        };
        let meta = div()
            .id("toggle-option-meta")
            .switch("Use Option as Meta", s.option_as_meta)
            .cursor_pointer()
            .on_click(cx.listener(|s, _, _, cx| {
                s.update_settings(|st| st.option_as_meta = !st.option_as_meta, cx);
                s.apply_option_as_meta(cx);
            }))
            .child(w::toggle(&t, s.option_as_meta, "option-meta"));

        let mut shapes = div().flex().items_center().gap(px(6.));
        for (ix, (style, label)) in [
            (CursorStyle::Block, "Block"),
            (CursorStyle::Bar, "Bar"),
            (CursorStyle::Underline, "Underline"),
        ]
        .into_iter()
        .enumerate()
        {
            let selected = s.cursor_style == style;
            shapes = shapes.child(
                w::button(&t, ("cursor-style", ix), label)
                    .when(selected, |el| el.bg(t.ink(0.16)).text_color(t.text))
                    .when(!selected, |el| el.text_color(t.muted))
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.change_terminal(|st| st.cursor_style = style, cx)
                    })),
            );
        }

        let lines = s.scrollback_lines;
        let (minus, value, plus) =
            w::stepper(&t, "scrollback", format!("{} lines", thousands(lines)));
        let scrollback = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(minus.on_click(cx.listener(|shell, _, _, cx| {
                shell.change_terminal(|st| st.step_scrollback(false), cx)
            })))
            .child(value)
            .child(plus.on_click(cx.listener(|shell, _, _, cx| {
                shell.change_terminal(|st| st.step_scrollback(true), cx)
            })));

        let reopen = div()
            .id("toggle-reopen-tabs")
            .cursor_pointer()
            .on_click(cx.listener(|s, _, _, cx| {
                s.update_settings(|st| st.reopen_tabs = !st.reopen_tabs, cx);
                s.persist_tabs();
            }))
            .child(w::toggle(&t, s.reopen_tabs, "reopen-tabs"));

        let log_all = div()
            .id("toggle-log-sessions")
            .cursor_pointer()
            .on_click(cx.listener(|s, _, _, cx| {
                s.update_settings(|st| st.log_sessions = !st.log_sessions, cx);
            }))
            .child(w::toggle(&t, s.log_sessions, "log-sessions"));

        let family = s
            .terminal_font_family
            .clone()
            .unwrap_or_else(|| crate::theme::MONO_FONT.to_owned());
        let font =
            w::button(&t, "terminal-font", family).on_click(cx.listener(
                |shell, e: &gpui::ClickEvent, _, cx| shell.open_font_menu(e.position(), cx),
            ));
        w::page_column()
            .child(w::page_header(&t, "Terminal"))
            .child(w::section(
                &t,
                "Font",
                w::card(&t).child(w::row(
                    &t,
                    true,
                    "Font",
                    Some("Monospace fonts on this Mac; a Nerd Font \"Mono\" keeps icons one cell wide".into()),
                    font,
                )),
            ))
            .child(w::section(
                &t,
                "Logging",
                w::card(&t).child(w::row(
                    &t,
                    true,
                    "Log every session",
                    Some(
                        format!(
                            "New tabs write plain text to {}/sessions; or start one from a tab's menu",
                            crate::settings::log_dir_label()
                        )
                        .into(),
                    ),
                    log_all,
                )),
            ))
            .child(w::section(
                &t,
                "Startup",
                w::card(&t).child(w::row(
                    &t,
                    true,
                    "Reopen tabs on launch",
                    Some(
                        "Connections come back waiting for Enter, so none dials until you ask"
                            .into(),
                    ),
                    reopen,
                )),
            ))
            .child(w::section(
                &t,
                "Cursor",
                w::card(&t)
                    .child(w::row(
                        &t,
                        true,
                        "Shape",
                        Some("Until the program on the remote asks for its own".into()),
                        shapes,
                    ))
                    .child(w::row(
                        &t,
                        false,
                        "Blink",
                        None,
                        toggle("cursor-blink", "Cursor blink", s.cursor_blink, |st| {
                            st.cursor_blink = !st.cursor_blink
                        }),
                    )),
            ))
            .child(w::section(
                &t,
                "Output",
                w::card(&t).child(w::row(
                    &t,
                    true,
                    "Scrollback",
                    Some(if cfg!(target_os = "macos") {
                        "Lines kept per terminal; ⌘F searches them"
                    } else {
                        "Lines kept per terminal; Ctrl+Shift+F searches them"
                    }.into()),
                    scrollback,
                )),
            ))
            .child(w::section(
                &t,
                "Keyboard and mouse",
                w::card(&t)
                    .child(w::row(
                        &t,
                        true,
                        "Use Option as Meta",
                        Some(
                            "⌥B, ⌥F and friends reach the shell; off types the macOS character"
                                .into(),
                        ),
                        meta,
                    ))
                    .child(w::row(
                        &t,
                        false,
                        "Copy on select",
                        Some("Finishing a selection copies it".into()),
                        toggle("copy-on-select", "Copy on select", s.copy_on_select, |st| {
                            st.copy_on_select = !st.copy_on_select
                        }),
                    ))
                    .child(w::row(
                        &t,
                        false,
                        "Middle-click pastes",
                        None,
                        toggle(
                            "middle-paste",
                            "Middle-click pastes",
                            s.middle_click_paste,
                            |st| st.middle_click_paste = !st.middle_click_paste,
                        ),
                    )),
            ))
            .child(w::section(
                &t,
                "Bell",
                w::card(&t)
                    .child(w::row(
                        &t,
                        true,
                        "Flash the terminal",
                        None,
                        toggle(
                            "visual-bell",
                            "Flash the terminal on bell",
                            s.visual_bell,
                            |st| st.visual_bell = !st.visual_bell,
                        ),
                    ))
                    .child(w::row(
                        &t,
                        false,
                        "Bounce the Dock icon",
                        Some("When tern is in the background".into()),
                        toggle(
                            "bell-dock",
                            "Bounce the Dock icon on bell",
                            s.bell_bounces_dock,
                            |st| st.bell_bounces_dock = !st.bell_bounces_dock,
                        ),
                    )),
            ))
    }
}

/// 10000 → "10,000", as the stepper reads at a glance.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn groups_digits_in_threes() {
        assert_eq!(super::thousands(1_000), "1,000");
        assert_eq!(super::thousands(100_000), "100,000");
        assert_eq!(super::thousands(999), "999");
    }
}
