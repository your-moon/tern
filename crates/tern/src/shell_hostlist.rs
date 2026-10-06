// Adapted from zeron crates/ui/src/popover.rs (search_input_frame) (MIT), see sidebar.rs.
//! The sidebar's moving parts that are not drawing: the search box, the recent hosts, which
//! group folders are folded, and the order the ⌘K picker lists hosts in.

use gpui::{AppContext, Context, Entity, Focusable, Subscription, Window, actions};

use super::Shell;
use crate::connections::{self, Connection};
use crate::picker;
use crate::recent::{self, Recent};
use crate::sidebar;
use crate::text_input::{InputColors, TextInput};
use crate::theme::Theme;

actions!(tern, [FocusHostSearch]);

pub(super) struct HostList {
    pub search: Entity<TextInput>,
    pub collapsed_groups: Vec<String>,
    pub recent: Recent,
    _repaint: Subscription,
}

impl HostList {
    pub fn new(t: &Theme, recent: Recent, cx: &mut Context<Shell>) -> Self {
        let colors = InputColors {
            text: t.text,
            placeholder: t.faint,
            cursor: t.accent,
            selection: t.accent.opacity(0.35),
        };
        let search = cx.new(|cx| TextInput::new("Search hosts", false, colors, cx));
        // The list under the box re-filters on every keystroke.
        let repaint = cx.observe(&search, |_, _, cx| cx.notify());
        Self {
            search,
            collapsed_groups: Vec::new(),
            recent,
            _repaint: repaint,
        }
    }
}

impl Shell {
    pub(super) fn search_query(&self, cx: &gpui::App) -> String {
        self.hostlist.search.read(cx).text().to_owned()
    }

    /// Moves the caret into the sidebar's search box, opening the sidebar and leaving
    /// Settings first when needed.
    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page = None;
        self.picker = None;
        if self.settings.sidebar_collapsed {
            self.toggle_sidebar(cx);
        }
        let focus = self.hostlist.search.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.hostlist
            .search
            .update(cx, |input, cx| input.set_text("", cx));
    }

    /// Escape clears the query and gives the terminal its keys back; Enter connects to the
    /// top match.
    pub(crate) fn on_search_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => {
                self.clear_search(cx);
                self.restore_focus(window, cx);
            }
            "enter" => {
                let query = self.search_query(cx);
                let top = sidebar::search(&query, &self.connections).first().copied();
                if let Some(host) = top.and_then(|ix| self.hosts.get(ix).cloned()) {
                    self.clear_search(cx);
                    self.connect_host(host, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(crate) fn toggle_group(&mut self, name: &str, cx: &mut Context<Self>) {
        let folded = &mut self.hostlist.collapsed_groups;
        match folded.iter().position(|g| g == name) {
            Some(at) => {
                folded.remove(at);
            }
            None => folded.push(name.to_owned()),
        }
        cx.notify();
    }

    /// Notes the connection time and writes `recent.json`; a failed write is only logged.
    pub(super) fn record_recent(&mut self, alias: &str) {
        self.hostlist.recent.touch(alias, recent::now());
        if let Some(dir) = crate::settings::dir()
            && let Err(e) = self.hostlist.recent.save(&dir)
        {
            tracing::warn!(error = %e, "recent_save_failed");
        }
    }

    /// Indices into `hosts` of the sidebar's Recent section, newest first.
    pub(super) fn recent_indices(&self) -> Vec<usize> {
        self.hostlist
            .recent
            .aliases()
            .filter_map(|a| self.hosts.iter().position(|h| h.alias == a))
            .take(recent::SIDEBAR_SHOWN)
            .collect()
    }

    /// Hosts the ⌘K picker lists for its current query, in order: the ranked matches, or on
    /// an empty query the recents first.
    pub fn picker_matches(&self) -> Vec<usize> {
        let labels: Vec<String> = self.connections.iter().map(sidebar::search_label).collect();
        let aliases: Vec<&str> = self.hosts.iter().map(|h| h.alias.as_str()).collect();
        let recent: Vec<&str> = self.hostlist.recent.aliases().collect();
        picker::order(&self.picker_query(), &labels, &aliases, &recent)
    }

    pub fn recent_aliases(&self) -> Vec<String> {
        self.hostlist.recent.aliases().map(str::to_owned).collect()
    }
}

impl Shell {
    /// Adds or replaces a connection and writes `hosts.json`.
    pub(super) fn store_connection(
        &mut self,
        editing: Option<usize>,
        connection: Connection,
    ) -> Result<(), String> {
        // A command typed in this form is approved by saving it.
        crate::password_command::approve_saved(&connection);
        let mut next = self.connections.clone();
        match editing.and_then(|ix| next.get_mut(ix)) {
            Some(slot) => *slot = connection,
            None => next.push(connection),
        }
        if let Some(dir) = crate::settings::dir() {
            connections::save(&dir, &next).map_err(|e| e.to_string())?;
        }
        self.connections = next;
        self.refresh_hosts();
        Ok(())
    }
}
