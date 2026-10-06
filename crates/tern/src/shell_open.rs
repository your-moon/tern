//! Opening, switching and closing tabs: a host becomes a tab with one or more panes, each its
//! own SSH session.

use gpui::{Context, Window};
use tern_ssh::{ConnectSpec, HostEntry};

use super::*;

impl Shell {
    /// Switches to the host's tab if it has one, reconnecting it when closed; otherwise opens
    /// a new tab.
    pub fn connect_host(&mut self, host: HostEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.record_recent(&host.alias);
        if let Some(ix) = self.tabs.iter().position(|tab| tab.alias == host.alias) {
            let session = self.tabs[ix].session().clone();
            session.update(cx, |s, cx| {
                if s.status.is_dormant() {
                    s.reconnect(cx);
                }
            });
            return self.activate_tab(ix, window, cx);
        }
        match ConnectSpec::from_host_entry(&host) {
            Ok(spec) => self.open_tab(Launch::Ssh(spec), host.alias, window, cx),
            Err(e) => self.fail(e.to_string(), cx),
        }
    }

    /// `tern <target>`: a `~/.ssh/config` alias or `user@host:port`.
    pub fn connect_target(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        match ConnectSpec::parse(target) {
            Ok(spec) => self.open_tab(Launch::Ssh(spec), target.to_string(), window, cx),
            Err(e) => self.fail(e.to_string(), cx),
        }
    }

    pub(super) fn open_tab(
        &mut self,
        launch: Launch,
        alias: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.make_pane(launch, &alias, window, cx);
        self.next_tab += 1;
        self.tabs.push(Tab {
            id: self.next_tab,
            alias,
            title: None,
            tree: split::Node::Leaf(pane.id),
            focused: pane.id,
            panes: vec![pane],
        });
        self.error = None;
        self.activate_tab(self.tabs.len() - 1, window, cx);
    }

    /// A session with the settings applied, and an eye on it for the day it drops.
    pub(super) fn make_pane(
        &mut self,
        launch: Launch,
        alias: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pane {
        // Debug builds: keep scripted logins out of the real known_hosts file.
        #[cfg(debug_assertions)]
        let launch = match launch {
            Launch::Ssh(spec) => Launch::Ssh(with_dev_known_hosts(spec)),
            Launch::SshIdle(spec) => Launch::SshIdle(with_dev_known_hosts(spec)),
        };
        let theme = self.terminal_theme(alias);
        let auth = self.auth_for(alias);
        let session = Session::open(
            launch,
            auth,
            alias.to_owned(),
            self.settings.log_sessions,
            theme,
            window,
            cx,
        );
        let meta = self.settings.option_as_meta;
        let view = session.read(cx).view.clone();
        let options = self.terminal_options();
        view.update(cx, |v, cx| {
            v.set_option_as_meta(meta);
            v.set_options(options, cx);
        });
        let bell = self.on_bell(&session, window, cx);
        // A tab that is not in front can drop without anyone seeing its terminal; say so, with
        // a way to get to it.
        let mut last = Status::Connecting;
        let repaint =
            cx.observe(&session, move |shell, session, cx| {
                let status = session.read(cx).status.clone();
                if status == Status::Closed && last != Status::Closed {
                    let ix = shell
                        .tabs
                        .iter()
                        .position(|t| t.panes.iter().any(|p| p.session == session));
                    if let Some(ix) =
                        ix.filter(|ix| *ix != shell.active || shell.settings_page.is_some())
                    {
                        let alias = shell.tabs[ix].alias.clone();
                        shell.toast(
                            Toast::new(ToastKind::Critical, format!("{alias} disconnected"))
                                .action("Show", move |s, window, cx| {
                                    if let Some(ix) = s.tabs.iter().position(|t| t.alias == alias) {
                                        s.settings_page = None;
                                        s.activate_tab(ix, window, cx);
                                    }
                                }),
                            cx,
                        );
                    }
                }
                last = status;
                cx.notify();
            });
        let notes = cx.subscribe(
            &session,
            |shell, _, note: &crate::session::SessionNote, cx| shell.on_session_note(note, cx),
        );
        self.next_pane += 1;
        Pane {
            id: self.next_pane,
            session,
            _repaint: repaint,
            _bell: bell,
            _notes: notes,
        }
    }

    pub fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        self.active = ix;
        self.persist_tabs();
        self.tab_scroll.scroll_to_item(ix);
        let focus = tab.session().read(cx).view.focus_handle(cx);
        window.focus(&focus, cx);
        self.apply_broadcast(cx);
        cx.notify();
    }

    /// Closing a tab drops its session, which ends the connection.
    pub fn close_tab_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.renaming = None;
        self.tabs.remove(ix);
        self.sync_broadcast(cx);
        if self.tabs.is_empty() {
            self.active = 0;
            self.persist_tabs();
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let next = if self.active > ix || self.active == self.tabs.len() {
            self.active - 1
        } else {
            self.active
        };
        self.activate_tab(next, window, cx);
    }
}
