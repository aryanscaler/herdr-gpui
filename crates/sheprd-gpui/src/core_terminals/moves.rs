//! Moving a herdr-core pane to a new tab, into another herdr-core tab of its
//! workspace, or to another workspace of its host, and moving a whole tab
//! to another workspace, as Herdr moves its panes and tabs. The terminal
//! keeps running: only the layouts that show it change, and the pane keeps
//! its name. A tab left without panes goes away without ending anything.

use super::{
    Host, TabPanes,
    menu::Target,
    panes::{Axis, Panes},
};
use crate::{
    SheprdWindow,
    browser::{Location, Store, TabId},
    i18n::fl,
    menu::Page,
    window::Flash,
};
use gpui::{prelude::*, *};
use sheprd_backend::TerminalId;

/// Where a pane or tab goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    /// A new tab of the pane's workspace.
    NewTab,
    /// A new workspace the app keeps on this computer.
    NewWorkspace,
    /// A new tab of another workspace of the host, by its ID.
    Workspace(String),
    /// Beside the pane in use of another tab, on its right.
    Tab(TabId),
}

/// One row of the move picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Move {
    pub(crate) destination: Destination,
    /// The tab, the workspace, or "New Tab".
    pub(crate) place: String,
}

impl SheprdWindow {
    /// Where `terminal`, a pane of `tab`, can go: a new tab unless it is the
    /// tab's only pane, a new workspace, every other herdr-core tab of the
    /// workspace that types into its terminals and has room, then every
    /// other workspace of the host.
    pub(crate) fn core_moves(&self, tab: TabId, terminal: &TerminalId, cx: &App) -> Vec<Move> {
        let Some(shown) = self.core_terminals.tabs.get(&tab) else {
            return Vec::new();
        };
        if shown.observe || !shown.panes.contains(terminal) {
            return Vec::new();
        }
        let mut moves = Vec::new();
        if shown.panes.count() > 1 {
            moves.push(Move {
                destination: Destination::NewTab,
                place: fl!("pane-menu-new-tab"),
            });
        }
        let (Some(store), Some((scope, workspace))) =
            (cx.try_global::<Store>(), self.browser_key())
        else {
            return moves;
        };
        if self.can_keep_workspace_for(&shown.host) {
            moves.push(Move {
                destination: Destination::NewWorkspace,
                place: fl!("pane-menu-new-workspace"),
            });
        }
        moves.extend(
            store
                .in_workspace(&scope, &workspace)
                .filter(|other| other.id != tab)
                .filter(|other| {
                    matches!(
                        &other.location,
                        Some(Location::CoreTerminal { panes, observe: false, .. })
                            if panes.count() < super::panes::MAX_PANES
                    )
                })
                .map(|other| Move {
                    destination: Destination::Tab(other.id),
                    place: other.label(),
                }),
        );
        moves.extend(self.other_workspaces(&workspace));
        moves
    }

    /// Where `tab` can go as a whole: a new workspace, then every other
    /// workspace of the host. A mirror only watches, so it stays.
    pub(crate) fn core_tab_moves(&self, tab: TabId) -> Vec<Move> {
        let Some(shown) = self.core_terminals.tabs.get(&tab) else {
            return Vec::new();
        };
        let Some((_, workspace)) = self.browser_key() else {
            return Vec::new();
        };
        if shown.observe {
            return Vec::new();
        }
        let mut moves = Vec::new();
        if self.can_keep_workspace_for(&shown.host) {
            moves.push(Move {
                destination: Destination::NewWorkspace,
                place: fl!("pane-menu-new-workspace"),
            });
        }
        moves.extend(self.other_workspaces(&workspace));
        moves
    }

    /// Whether a new workspace kept for a tab on `host` would share its
    /// core: the app keeps new workspaces on this computer only.
    fn can_keep_workspace_for(&self, host: &Host) -> bool {
        *host == Host::Local && !self.selected_is_remote()
    }

    /// The selected host's workspaces other than `workspace`, Herdr 1's then
    /// the app's, each a place for a new tab.
    fn other_workspaces(&self, workspace: &str) -> Vec<Move> {
        let sessions = self.sessions(self.selected_endpoint);
        sessions
            .each()
            .flat_map(|session| &session.workspaces)
            .filter(|other| other.id != workspace)
            .map(|other| Move {
                destination: Destination::Workspace(other.id.clone()),
                place: other.label.clone(),
            })
            .collect()
    }

    /// Opens the move picker of `terminal` in `tab`, as the pane menu's
    /// "Move to…" row and the Move Pane command do. With nowhere to go, it
    /// says so instead.
    pub(crate) fn open_core_move(
        &mut self,
        tab: TabId,
        terminal: TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.core_moves(tab, &terminal, cx).is_empty() {
            self.show_flash(Flash::warning(fl!("core-terminals-nowhere-to-move")), cx);
            return;
        }
        self.open_core_picker(Target::Pane { tab, terminal }, window, cx);
    }

    /// Opens the workspace picker of `tab`, as its menu's "Move to
    /// Workspace" row does.
    pub(crate) fn open_core_tab_move(
        &mut self,
        tab: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.core_tab_moves(tab).is_empty() {
            self.show_flash(Flash::warning(fl!("core-terminals-nowhere-to-move")), cx);
            return;
        }
        self.open_core_picker(Target::Tab(tab), window, cx);
    }

    fn open_core_picker(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.page != Some(Page::CoreMenu) {
            self.open_core_menu(target, self.bounds.origin, window, cx);
        }
        self.menu.page = Some(Page::CoreMove);
        self.reset_core_selection();
        cx.notify();
    }

    /// Moves `terminal` out of `tab` to `destination`, shows it there with
    /// the keyboard, and saves both layouts.
    pub(crate) fn move_core_pane(
        &mut self,
        tab: TabId,
        terminal: TerminalId,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Another window may have saved the tab since this one read it, as
        // when it moved the other pane out: where this pane can go, and
        // whether it is alone, are read from that.
        self.sync_core_views(cx);
        if !self
            .core_moves(tab, &terminal, cx)
            .iter()
            .any(|row| row.destination == destination)
        {
            return;
        }
        let Some(shown) = self.core_terminals.tabs.get(&tab) else {
            return;
        };
        // The name goes with the pane once it has somewhere to go.
        let name = shown.names.get(&terminal).cloned();
        let alone = shown.panes.count() == 1;
        let to = match &destination {
            Destination::Tab(to) => *to,
            Destination::NewTab | Destination::NewWorkspace | Destination::Workspace(_) => {
                let cwd = self.core_folder(tab, &terminal);
                let Some(workspace) = self.destination_workspace(&destination, cwd, cx) else {
                    return;
                };
                let Some((scope, _)) = self.browser_key() else {
                    return;
                };
                let location = Location::core_terminal(Panes::single(terminal.clone()), false);
                let Some(id) = Store::update(cx, |store| {
                    store.open(scope, &workspace, Some(location), None)
                }) else {
                    self.show_flash(Flash::warning(fl!("core-terminals-too-many-tabs")), cx);
                    return;
                };
                id
            }
        };
        // Either tab may not be shown yet: it is read from its saved layout.
        self.sync_core_views(cx);
        self.load_core_tab(to, cx);
        let Some(other) = self.core_terminals.tabs.get_mut(&to) else {
            return;
        };
        if let Destination::Tab(_) = destination {
            let at = other.active.clone();
            if let Err(error) = other.panes.split(&at, Axis::Row, terminal.clone()) {
                self.show_flash(Flash::warning(error.to_string()), cx);
                return;
            }
            other.activate(terminal.clone());
            other.set_zoom(None);
        }
        if let Some(name) = name {
            other.names.insert(terminal.clone(), name);
        }
        other.unsaved = true;
        // Saved before the old place lets go, so nothing reads the terminal
        // as left behind and ends it.
        self.save_core_panes(cx);
        if alone {
            self.forget_core_tab(tab, window, cx);
        } else {
            if let Some(shown) = self.core_terminals.tabs.get_mut(&tab) {
                shown.names.remove(&terminal);
            }
            self.remove_core_pane(tab, &terminal, window, cx);
            self.save_core_panes(cx);
        }
        self.sync_core_views(cx);
        self.reveal_moved_core_tab(to, &terminal, window, cx);
    }

    /// Moves `tab` as a whole to `destination`, a workspace, and shows it
    /// there. Its terminals keep running and its layout is unchanged.
    pub(crate) fn move_core_tab(
        &mut self,
        tab: TabId,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_core_views(cx);
        if !self
            .core_tab_moves(tab)
            .iter()
            .any(|row| row.destination == destination)
        {
            return;
        }
        let Some(active) = self
            .core_terminals
            .tabs
            .get(&tab)
            .map(|shown| shown.active.clone())
        else {
            return;
        };
        let cwd = self.core_folder(tab, &active);
        let Some(workspace) = self.destination_workspace(&destination, cwd, cx) else {
            return;
        };
        if !Store::update(cx, |store| store.move_to_workspace(tab, &workspace)) {
            return;
        }
        if let Some((scope, _)) = self.browser_key()
            && let Some(shown) = self.core_terminals.tabs.get_mut(&tab)
        {
            shown.workspace = Some((scope, workspace));
        }
        self.sync_core_views(cx);
        self.reveal_moved_core_tab(tab, &active, window, cx);
    }

    /// The workspace a move to `destination` lands in: a new one the app
    /// keeps, in the folder `cwd` the pane was in, or the one named.
    fn destination_workspace(
        &mut self,
        destination: &Destination,
        cwd: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        match destination {
            Destination::NewTab => self.browser_key().map(|(_, workspace)| workspace),
            Destination::Workspace(workspace) => Some(workspace.clone()),
            // Kept and brought to the front, where the tab then shows.
            Destination::NewWorkspace => self.keep_new_workspace(None, cwd.as_deref(), cx),
            Destination::Tab(_) => None,
        }
    }

    /// Reads `tab` from its saved layout when the window holds none for it,
    /// as for a tab of a workspace it has not shown.
    fn load_core_tab(&mut self, tab: TabId, cx: &App) {
        if self.core_terminals.tabs.contains_key(&tab) {
            return;
        }
        let Some(stored) = cx.try_global::<Store>().and_then(|store| store.get(tab)) else {
            return;
        };
        let Some(location) = &stored.location else {
            return;
        };
        let Some(mut restored) = TabPanes::restore(Host::of(&stored.scope), location) else {
            return;
        };
        restored.saved = Some(location.clone());
        self.core_terminals.tabs.insert(tab, restored);
    }

    /// Shows `to`, where a pane or tab just moved, with `terminal` in use,
    /// bringing its workspace to the front first when it is another one.
    fn reveal_moved_core_tab(
        &mut self,
        to: TabId,
        terminal: &TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = cx
            .try_global::<Store>()
            .and_then(|store| store.get(to))
            .map(|tab| tab.workspace_id.clone());
        let Some(workspace) = workspace else {
            return;
        };
        let endpoint = self.endpoints[self.selected_endpoint].id.clone();
        self.show_core_agent(
            &endpoint,
            &workspace,
            &to.to_string(),
            terminal.as_str(),
            window,
            cx,
        );
        self.focus_core_terminal(to, window, cx);
        cx.notify();
    }

    /// Takes `tab` away once a move emptied it. Closed as a page is, so its
    /// group moves on; its terminal is not ended, as the tab it moved to
    /// holds it now.
    fn forget_core_tab(&mut self, tab: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.core_terminals.tabs.remove(&tab);
        self.core_terminals
            .views
            .retain(|(shown, _), _| *shown != tab);
        self.close_browser_tab(tab, window, cx);
    }
}
