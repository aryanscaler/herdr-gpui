//! Moving a herdr-core pane to a new tab, or into another herdr-core tab of
//! its workspace, as Herdr moves its panes. The terminal keeps running: only
//! the layouts that show it change, and the pane keeps its name. A tab left
//! without panes goes away without ending anything.

use super::{
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

/// Where a pane goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    /// A new tab of the pane's workspace.
    NewTab,
    /// Beside the pane in use of another tab, on its right.
    Tab(TabId),
}

/// One row of the move picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Move {
    pub(crate) destination: Destination,
    /// The tab, or "New Tab".
    pub(crate) place: String,
}

impl SheprdWindow {
    /// Where `terminal`, a pane of `tab`, can go: a new tab unless it is the
    /// tab's only pane, then every other herdr-core tab of the workspace that
    /// types into its terminals and has room.
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
        moves
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
        let target = Target::Pane { tab, terminal };
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
        let Some(shown) = self.core_terminals.tabs.get_mut(&tab) else {
            return;
        };
        // The name goes with the pane once it has somewhere to go.
        let name = shown.names.get(&terminal).cloned();
        let alone = shown.panes.count() == 1;
        let to = match destination {
            Destination::NewTab => {
                let Some((scope, workspace)) = self.browser_key() else {
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
            Destination::Tab(to) => to,
        };
        // Either tab may not be shown yet: it is read from its saved layout.
        self.sync_core_views(cx);
        let Some(other) = self.core_terminals.tabs.get_mut(&to) else {
            return;
        };
        if destination != Destination::NewTab {
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
        self.show_browser_tab(to, window, cx);
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
