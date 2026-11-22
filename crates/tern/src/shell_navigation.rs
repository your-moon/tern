// Adapted from zeron apps/zgx/ui/screens/shell/route.luau and ds/sidebar.luau (MIT).
use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Destination {
    Workspace(Option<broadcast_ui::TabId>),
    Settings(settings_ui::Section),
    Connection(String),
}

impl Shell {
    // Zeron shell/route.luau replaces the Settings entry when switching sections.
    pub(super) fn remember_destination(&mut self) {
        let current = self.settings_page.map_or_else(
            || match &self.workspaces.empty {
                Some(alias) => Destination::Connection(alias.clone()),
                None => Destination::Workspace(self.tabs.get(self.active).map(|tab| tab.id)),
            },
            Destination::Settings,
        );
        if self.nav_tab_count == self.tabs.len()
            && self.navigation.get(self.nav_index) == Some(&current)
        {
            return;
        }
        self.nav_tab_count = self.tabs.len();
        let live = |entry: &Destination| match entry {
            Destination::Workspace(Some(id)) => self.tabs.iter().any(|tab| tab.id == *id),
            Destination::Workspace(None) => true,
            Destination::Settings(_) => true,
            Destination::Connection(alias) => self.hosts.iter().any(|host| &host.alias == alias),
        };
        let before = self
            .navigation
            .iter()
            .take(self.nav_index)
            .filter(|entry| live(entry))
            .count();
        let retained = self.navigation.get(self.nav_index).is_some_and(live);
        self.navigation.retain(live);
        self.nav_index = before;
        if retained && let Some(entry) = self.navigation.get_mut(before) {
            if *entry == current {
                return;
            }
            if matches!(
                (&*entry, &current),
                (Destination::Settings(_), Destination::Settings(_))
            ) {
                *entry = current;
                return;
            }
        }
        self.navigation.truncate(before + usize::from(retained));
        if self.navigation.last() != Some(&current) {
            self.navigation.push(current);
        }
        self.nav_index = self.navigation.len() - 1;
    }

    pub(crate) fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let next = if forward {
            self.nav_index.checked_add(1)
        } else {
            self.nav_index.checked_sub(1)
        };
        let Some((index, destination)) =
            next.and_then(|index| self.navigation.get(index).cloned().map(|d| (index, d)))
        else {
            return;
        };
        self.nav_index = index;
        match destination {
            Destination::Connection(alias) => self.restore_connection_view(&alias, window, cx),
            Destination::Settings(section) => {
                self.select_section(section, cx);
                window.focus(&self.focus, cx);
            }
            Destination::Workspace(Some(id)) => {
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    self.show_tab(index, window, cx);
                }
            }
            Destination::Workspace(None) => self.show_connections(window, cx),
        }
    }

    pub(super) fn sidebar_target(&self) -> f32 {
        if self.settings.sidebar_collapsed {
            0.0
        } else {
            look::snap(self.settings.sidebar_width, self.frame.scale)
        }
    }

    /// The sidebar's width this frame: mid-tween while collapsing or expanding.
    pub(super) fn sidebar_now(&self) -> f32 {
        self.sidebar_tween
            .and_then(|tween| tween.sample(std::time::Instant::now()))
            .map_or_else(
                || self.sidebar_target(),
                |w| look::snap(w, self.frame.scale),
            )
    }

    pub fn toggle_sidebar(&mut self, animate: bool, cx: &mut Context<Self>) {
        if self.showing_intro() {
            self.intro_entered = true;
            self.update_settings(|s| s.sidebar_collapsed = false, cx);
            return;
        }
        let from = self.sidebar_now();
        self.update_settings(|s| s.sidebar_collapsed = !s.sidebar_collapsed, cx);
        self.sidebar_tween =
            (animate && !cx.reduce_motion()).then(|| WidthTween::new(from, self.sidebar_target()));
    }
}
