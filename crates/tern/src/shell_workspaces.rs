//! One workspace per connection alias. Tabs retain their global indices for pane actions.
use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Workspaces {
    pub(super) empty: Option<String>,
    pub(super) last_session: BTreeMap<String, broadcast_ui::TabId>,
    pub(super) collapsed: Vec<String>,
}

impl Shell {
    pub(super) fn workspace_alias(&self) -> Option<&str> {
        self.tabs
            .get(self.active)
            .map(|tab| tab.alias.as_str())
            .or(self.workspaces.empty.as_deref())
    }

    pub(super) fn remembered_session(&self, alias: &str) -> Option<usize> {
        let last = self.workspaces.last_session.get(alias);
        self.tabs
            .iter()
            .position(|tab| tab.alias == alias && Some(&tab.id) == last)
            .or_else(|| self.tabs.iter().position(|tab| tab.alias == alias))
    }

    pub(crate) fn select_connection(
        &mut self,
        alias: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self.hosts.iter().find(|host| host.alias == alias).cloned() {
            self.connect_host(host, window, cx);
        } else if let Some(ix) = self.remembered_session(alias) {
            self.show_tab(ix, window, cx);
        }
    }

    pub(super) fn restore_connection_view(
        &mut self,
        alias: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_settings(window, cx);
        self.renaming = None;
        if let Some(ix) = self.remembered_session(alias) {
            self.activate_tab(ix, window, cx);
        } else {
            self.active = self.tabs.len();
            self.workspaces.empty = self
                .hosts
                .iter()
                .any(|host| host.alias == alias)
                .then(|| alias.to_owned());
            self.persist_tabs();
            self.restore_focus(window, cx);
        }
    }

    pub(crate) fn toggle_connection_group(&mut self, alias: &str, cx: &mut Context<Self>) {
        let groups = &mut self.workspaces.collapsed;
        if let Some(at) = groups.iter().position(|s| s == alias) {
            groups.remove(at);
        } else {
            groups.push(alias.to_owned());
        }
        cx.notify();
    }

    pub(crate) fn new_workspace_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(alias) = self.workspace_alias().map(str::to_owned) else {
            return self.toggle_picker(window, cx);
        };
        self.new_session_for(&alias, window, cx);
    }

    pub(super) fn new_session_for(
        &mut self,
        alias: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self.hosts.iter().find(|host| host.alias == alias).cloned() {
            self.open_new_tab(&host, window, cx);
        } else if let Some(ix) = self.remembered_session(alias) {
            let launch = self.tabs[ix].session().read(cx).launch_again();
            self.open_tab(launch, alias.to_owned(), window, cx);
        }
    }

    pub(super) fn workspace_tab(
        &mut self,
        slot: Option<usize>,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let indices: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| Some(tab.alias.as_str()) == self.workspace_alias())
            .map(|(ix, _)| ix)
            .collect();
        let current = indices
            .iter()
            .position(|ix| *ix == self.active)
            .unwrap_or(0);
        let next = slot.unwrap_or_else(|| tabs::step(current, indices.len(), direction));
        if let Some(&ix) = indices.get(next) {
            self.show_tab(ix, window, cx);
        }
    }

    pub(super) fn empty_workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                crate::settings_widgets::action_button(
                    &self.theme,
                    "open-workspace-terminal",
                    "Open terminal",
                    true,
                )
                .on_click(
                    cx.listener(|shell, _, window, cx| shell.new_workspace_session(window, cx)),
                ),
            )
            .into_any_element()
    }
}

impl Shell {
    pub(super) fn tab_infos(&self, cx: &App) -> Vec<TabInfo> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| TabInfo {
                alias: tab.alias.clone(),
                title: tab.title.clone(),
                logging: tab.session().read(cx).is_logging(),
                broadcast: self.is_broadcasting(tab.id),
                dot: {
                    let s = tab.session().read(cx);
                    Dot::of(&s.status, s.is_asking())
                },
                rename: self
                    .renaming
                    .as_ref()
                    .filter(|r| r.ix == ix)
                    .map(|r| r.input.clone()),
            })
            .collect()
    }
}
