//! Find-bar and link handling for [`TerminalView`]: a child module so it
//! can use the view's private state.

use super::*;

impl TerminalView {
    // ---- links ----
    //
    // Cmd (Ctrl elsewhere) underlines the link under the pointer; Cmd+click
    // opens it. Plain URLs and OSC 8 hyperlinks both count.

    /// The link at a window position, if the link modifier is held.
    pub(super) fn link_under(
        &self,
        position: gpui::Point<Pixels>,
        modifiers: &gpui::Modifiers,
        cx: &App,
    ) -> Option<Link> {
        if !modifiers.secondary() {
            return None;
        }
        let hit = self.hit(position)?;
        self.terminal.read(cx).link_at(hit.row, hit.col)
    }

    pub(super) fn update_hover(
        &mut self,
        position: gpui::Point<Pixels>,
        modifiers: &gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        self.last_pointer = Some(position);
        let link = self.link_under(position, modifiers, cx);
        if link != self.hover_link {
            self.hover_link = link;
            cx.notify();
        }
    }

    pub(super) fn on_modifiers_changed(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(position) = self.last_pointer {
            self.update_hover(position, &event.modifiers, cx);
        }
    }

    /// The link currently underlined, as viewport segments for the element.
    pub(crate) fn hover_link(&self) -> Option<&Link> {
        self.hover_link.as_ref()
    }

    // ---- find in scrollback ----
    //
    // The find bar is bound to Cmd+F (Ctrl+Shift+F elsewhere) while the view
    // has focus. Enter / Shift+Enter step through matches, Escape closes.

    /// Open the find bar on `query` and highlight every match, jumping to the
    /// newest. An empty query opens an empty bar.
    pub fn find(&mut self, query: &str, cx: &mut Context<Self>) {
        self.find = Some(query.to_string());
        self.terminal.update(cx, |t, cx| {
            t.search(query);
            cx.notify();
        });
        cx.notify();
    }

    /// The next match, going up into older output; wraps.
    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        self.step_find(true, cx);
    }

    /// The previous match, going down toward newer output; wraps.
    pub fn find_prev(&mut self, cx: &mut Context<Self>) {
        self.step_find(false, cx);
    }

    /// Close the find bar and drop the highlights.
    pub fn clear_find(&mut self, cx: &mut Context<Self>) {
        self.find = None;
        self.terminal.update(cx, |t, cx| {
            t.clear_search();
            cx.notify();
        });
        cx.notify();
    }

    /// Matches for the current query (0 when no search is open).
    pub fn match_count(&self, cx: &App) -> usize {
        self.terminal.read(cx).search_count()
    }

    /// Whether the find bar is open.
    pub fn is_finding(&self) -> bool {
        self.find.is_some()
    }

    fn step_find(&mut self, older: bool, cx: &mut Context<Self>) {
        self.terminal.update(cx, |t, cx| {
            t.search_step(older);
            cx.notify();
        });
    }

    /// Keystrokes while the find bar is open. Always swallowed.
    pub(super) fn on_find_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        let Some(query) = self.find.as_mut() else {
            return;
        };
        let mods = &ks.modifiers;
        let action = if ks.key == "v" && (mods.platform || (mods.control && mods.shift)) {
            match cx.read_from_clipboard().and_then(|item| item.text()) {
                Some(text) => paste_into(query, &text),
                None => FindKey::Ignored,
            }
        } else if ks.key == "g" && (mods.platform || mods.control) {
            if mods.shift {
                FindKey::Prev
            } else {
                FindKey::Next
            }
        } else {
            apply_key(query, ks)
        };
        match action {
            FindKey::Close => self.clear_find(cx),
            FindKey::Next => self.find_next(cx),
            FindKey::Prev => self.find_prev(cx),
            FindKey::Edited => {
                let query = self.find.clone().unwrap_or_default();
                self.find(&query, cx);
            }
            FindKey::Ignored => {}
        }
    }
}

/// Cmd+F (macOS) / Ctrl+Shift+F: open the find bar.
pub(super) fn is_find_chord(ks: &gpui::Keystroke) -> bool {
    let mods = &ks.modifiers;
    ks.key == "f" && (mods.platform || (mods.control && mods.shift))
}

/// "3/12", "No matches", or "" for an empty query.
pub(super) fn find_status(query: &str, count: usize, index: Option<usize>) -> String {
    match (query.is_empty(), count, index) {
        (true, ..) => String::new(),
        (false, 0, _) => "No matches".to_string(),
        (false, n, Some(i)) => format!("{}/{n}", i + 1),
        (false, n, None) => format!("{n}"),
    }
}
