//! Tab editing on the strip: renaming a title in place and dragging a tab to a new slot.

use gpui::{AppContext, Context, Entity, KeyDownEvent, Window};
use tern_ssh::ConnectSpec;

use super::Shell;
use crate::session::Launch;
use crate::tabs;
use crate::tabs_store::{SavedKind, SavedTab, SavedTabs};
use crate::text_input::{InputColors, TextInput};

/// A title being edited: which tab, and the field holding the text.
pub(super) struct Rename {
    pub ix: usize,
    pub input: Entity<TextInput>,
}

impl Shell {
    pub(crate) fn is_renaming(&self, ix: usize) -> bool {
        self.renaming.as_ref().is_some_and(|r| r.ix == ix)
    }

    /// Swaps the tab's title for a field holding it, selected for typing over.
    pub(crate) fn start_rename(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        let current = tab.title.clone().unwrap_or_else(|| tab.alias.clone());
        let t = self.theme;
        let colors = InputColors {
            text: t.text,
            placeholder: t.faint,
            cursor: t.accent,
            selection: t.accent.opacity(0.35),
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(tab.alias.clone(), false, colors, cx);
            input.set_text(current, cx);
            input.select_everything(cx);
            input
        });
        window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx), cx);
        self.renaming = Some(Rename { ix, input });
        cx.notify();
    }

    /// Saves the field. An empty title, or the alias itself, clears the custom title.
    pub(crate) fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        let typed = rename.input.read(cx).text().trim().to_owned();
        if let Some(tab) = self.tabs.get_mut(rename.ix) {
            tab.title = custom_title(&typed, &tab.alias);
        }
        self.persist_tabs();
        self.restore_focus(window, cx);
    }

    pub(crate) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            self.restore_focus(window, cx);
        }
    }

    /// Enter saves and Escape drops the edit; true when the key was for the field.
    pub(super) fn on_rename_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.renaming.is_none() {
            return false;
        }
        match event.keystroke.key.as_str() {
            "enter" => self.commit_rename(window, cx),
            "escape" => self.cancel_rename(window, cx),
            _ => return false,
        }
        true
    }

    /// A tab was dropped on slot `to`; the tab in front stays in front.
    pub(crate) fn move_tab(
        &mut self,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.renaming = None;
        let active = tabs::reorder(&mut self.tabs, self.active, from, to);
        self.activate_tab(active, window, cx);
    }
}

impl Shell {
    pub(crate) fn start_logging(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(session) = self.tabs.get(ix).map(|t| t.session().clone()) else {
            return;
        };
        match session.update(cx, |s, cx| {
            cx.notify();
            s.start_logging()
        }) {
            // The file name, not the full path (it ran past the toast), and a way to find it.
            Ok(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.toast(
                    super::Toast::new(super::ToastKind::Default, format!("Logging to {name}"))
                        .action(crate::platform::REVEAL_LABEL, move |_, _, cx| {
                            cx.reveal_path(&path)
                        }),
                    cx,
                );
            }
            Err(e) => self.notify_toast(
                super::ToastKind::Critical,
                format!("Could not start the log: {e}"),
                cx,
            ),
        }
    }

    pub(crate) fn stop_logging(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(session) = self.tabs.get(ix).map(|t| t.session().clone()) else {
            return;
        };
        let path = session.update(cx, |s, cx| {
            cx.notify();
            s.stop_logging()
        });
        if let Some(path) = path {
            self.notify_toast(
                super::ToastKind::Default,
                format!("Log saved to {}", path.display()),
                cx,
            );
        }
    }

    /// Writes the open tabs to `tabs.json`; a failed write is logged, not fatal. It runs on
    /// every change, never at launch, so an unreadable file survives until the user changes
    /// something.
    pub(crate) fn persist_tabs(&self) {
        if self.restoring || !self.settings.reopen_tabs {
            return;
        }
        let Some(dir) = crate::settings::dir() else {
            return;
        };
        if let Err(e) = self.saved_tabs().save(&dir) {
            tracing::warn!(error = %e, "tabs_save_failed");
        }
    }

    fn saved_tabs(&self) -> SavedTabs {
        SavedTabs {
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    kind: SavedKind::Ssh {
                        alias: tab.alias.clone(),
                    },
                    title: tab.title.clone(),
                })
                .collect(),
            active: self.active,
        }
    }

    /// Reopens the last run's tabs: connections wait for Enter.
    pub(crate) fn restore_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.reopen_tabs {
            return;
        }
        let Some(saved) = crate::settings::dir().and_then(|d| SavedTabs::load(&d)) else {
            return;
        };
        self.restoring = true;
        for saved_tab in &saved.tabs {
            let opened = match &saved_tab.kind {
                SavedKind::Ssh { alias } => self.reopen_idle(alias, window, cx),
                SavedKind::Unknown => false,
            };
            if opened {
                let title = saved_tab.title.clone();
                if let Some(tab) = self.tabs.last_mut() {
                    tab.title = title;
                }
            }
        }
        self.restoring = false;
        if !self.tabs.is_empty() {
            let active = saved.active_slot().min(self.tabs.len() - 1);
            self.activate_tab(active, window, cx);
        }
    }

    /// A connection tab that does not dial yet. A host that is gone from the lists is skipped.
    fn reopen_idle(&mut self, alias: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let spec = match self.hosts.iter().find(|h| h.alias == alias) {
            Some(host) => ConnectSpec::from_host_entry(host),
            None => ConnectSpec::parse(alias),
        };
        match spec {
            Ok(spec) => {
                self.open_tab(Launch::SshIdle(spec), alias.to_owned(), window, cx);
                true
            }
            Err(e) => {
                tracing::warn!(alias, error = %e, "tab_restore_skipped");
                false
            }
        }
    }
}

/// What a typed title means: nothing, or the alias it already shows, is no custom title.
fn custom_title(typed: &str, alias: &str) -> Option<String> {
    (!typed.is_empty() && typed != alias).then(|| typed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::custom_title;

    #[test]
    fn empty_or_unchanged_titles_are_not_custom() {
        assert_eq!(custom_title("", "prod"), None);
        assert_eq!(custom_title("prod", "prod"), None);
        assert_eq!(custom_title("prod db", "prod"), Some("prod db".into()));
    }
}
