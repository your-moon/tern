//! How the window looks: colour schemes per host, light and dark appearance, restyling open
//! terminals, and the status line under the terminal.

use gpui::{App, Context, IntoElement};

use super::*;

impl Shell {
    /// The scheme a host's tabs use: its own, else the default, else zeron's (`None`).
    pub(super) fn scheme_for(&self, alias: &str) -> Option<&'static crate::themes::Scheme> {
        self.settings
            .host_themes
            .get(alias)
            .or(self.settings.terminal_theme.as_ref())
            .and_then(|name| crate::themes::find(name))
    }

    /// Switches the palette when the chosen appearance (or the system's, for System) differs
    /// from the one showing, and restyles everything that holds a colour: open terminals, text
    /// fields, the wallpaper tint.
    pub(crate) fn apply_appearance(&mut self, cx: &mut Context<Self>) {
        let light = self.settings.appearance.is_light(self.system_light);
        if light == self.theme.light {
            return;
        }
        self.theme = Theme::zeron(light);
        self.refresh_theme(cx);
        self.install_input_colors(cx);
        self.restyle_tabs(cx);
        cx.notify();
    }

    /// Standard-density mode for this frame, from the monitor the window is on right now.
    pub(super) fn sync_density(&self, window: &Window) {
        crate::theme::set_low_dpi(
            self.settings.sharp_text && window.scale_factor() < crate::theme::LOW_DPI_BELOW,
        );
    }

    /// Text fields read their colours from this global, so open ones follow the theme.
    pub(crate) fn install_input_colors(&self, cx: &mut Context<Self>) {
        let t = self.theme;
        cx.set_global(crate::text_input::InputColors {
            text: t.text,
            placeholder: t.faint,
            cursor: t.accent,
            selection: t.accent.opacity(0.35),
        });
    }

    pub(super) fn terminal_theme(&self, alias: &str) -> tern_term::TerminalTheme {
        let mut theme = self
            .theme
            .terminal(self.settings.terminal_font_size, self.scheme_for(alias));
        if let Some(family) = &self.settings.terminal_font_family {
            theme.font_family = family.clone().into();
        }
        // Over a full-window wallpaper the terminal is a panel too: the picture shows through.
        if let Some(alpha) = self.window_fill() {
            theme.background_alpha = alpha;
        }
        theme
    }

    /// Re-applies font and scheme to every open terminal; each re-measures its cells and the
    /// remote side is told the new grid size.
    pub(super) fn restyle_tabs(&self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            for pane in &tab.panes {
                let theme = self.terminal_theme(&tab.alias);
                let view = pane.session.read(cx).view.clone();
                view.update(cx, |v, cx| v.set_theme(theme, cx));
            }
        }
    }

    pub(super) fn change_font(
        &mut self,
        change: impl FnOnce(&mut Settings),
        cx: &mut Context<Self>,
    ) {
        self.update_settings(change, cx);
        self.restyle_tabs(cx);
    }

    /// Background of the main panel: the active tab's scheme, so no seam shows around it.
    pub(super) fn panel_background(&self) -> gpui::Hsla {
        match self.tabs.get(self.active) {
            Some(tab) => self.terminal_theme(&tab.alias).background,
            None => self.theme.terminal_background,
        }
    }

    /// The strip under the active tab's terminal, when the setting is on.
    pub(super) fn render_status_line(
        &self,
        bg: gpui::Hsla,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        if !self.settings.show_status_line {
            return None;
        }
        let session = self.tabs.get(self.active)?.session().read(cx);
        Some(statusline::render(
            &self.theme,
            &session.label(),
            &session.status,
            session.elapsed(),
            bg,
        ))
    }

    /// Repaints once a second while the active tab is connected, so its session time counts.
    pub(super) fn start_clock(&mut self, cx: &mut Context<Self>) {
        self._clock = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let alive = this.update(cx, |s, cx| {
                    let ticking = s.settings.show_status_line
                        && s.tabs
                            .get(s.active)
                            .is_some_and(|t| t.session().read(cx).status == Status::Connected);
                    if ticking {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }
}

/// Text over the wallpaper hero sits on a plate of the panel colour, so it never depends on
/// what the picture holds.
pub(super) fn plate(el: gpui::Div, panel: gpui::Hsla) -> gpui::Div {
    use gpui::Styled;
    el.px(gpui::px(20.))
        .py(gpui::px(12.))
        .rounded(gpui::px(12.))
        .bg(panel.opacity(0.85))
}

impl Shell {
    /// The window's base: solid under a wallpaper (the frost let other windows show wherever the
    /// picture left a pixel uncovered), frost otherwise.
    pub(super) fn window_base(&self) -> gpui::Hsla {
        if self.has_wallpaper() {
            self.theme.shell
        } else {
            self.theme.surface()
        }
    }

    /// The main panel's frame. With a terminal open over a wallpaper it runs to the window's
    /// edges, unbordered: an inset there framed the terminal in a tinted outline with picture
    /// showing round it. Otherwise zeron's inset card with a hairline.
    pub(super) fn main_panel(&self, sidebar_now: f32) -> gpui::Div {
        use gpui::prelude::FluentBuilder;
        use gpui::{Styled, div, px};
        let t = self.theme;
        let flush = !self.tabs.is_empty() && self.window_fill().is_some();
        div()
            .flex_1()
            .min_w_0()
            .when(!flush, |el| {
                el.when(sidebar_now < crate::theme::SPACE_SM, |el| {
                    el.ml(px(crate::theme::SPACE_SM))
                })
                .mr(px(crate::theme::SPACE_SM))
                .mb(px(crate::theme::SPACE_SM))
                .rounded(px(crate::theme::PANEL_RADIUS))
                .border_1()
                .border_color(if self.window_fill().is_some() {
                    t.hairline
                } else {
                    t.border
                })
            })
    }
}
