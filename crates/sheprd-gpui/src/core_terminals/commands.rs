//! The window's pane commands, run on a herdr-core tab's panes while one
//! holds the keyboard: split, move focus, swap, resize, equalize, zoom, and
//! close. The same shortcuts work on Herdr panes, so a herdr-core tab
//! handles them here rather than letting them reach Herdr; one it cannot
//! run says so.

use super::{
    Purpose,
    panes::{Axis, Direction, Panes},
};
use crate::{
    SheprdWindow,
    browser::{Location, Store, TabId},
    controls::Command,
    i18n::fl,
    window::Flash,
};
use gpui::{App, Context, Window};
use sheprd_backend::TerminalId;

impl SheprdWindow {
    /// The herdr-core pane a pane command acts on: the one holding the
    /// keyboard, or whose chat does, else, while a workspace the app keeps
    /// is in front, the pane in use of the tab it shows, as when the
    /// keyboard is on the sidebar after stepping to the workspace.
    pub(crate) fn core_command_pane(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<(TabId, TerminalId)> {
        self.core_terminals
            .focused_pane(window, cx)
            .or_else(|| self.focused_core_chat(window, cx))
            .or_else(|| self.kept_front_pane(cx))
    }

    /// While a workspace the app keeps is in front, the pane in use of the
    /// herdr-core tab its active group shows.
    pub(crate) fn kept_front_pane(&self, cx: &App) -> Option<(TabId, TerminalId)> {
        self.kept_in_front()?;
        self.shown_core_pane(cx)
    }

    /// Runs `command` on the herdr-core tab's panes [`Self::core_command_pane`]
    /// names. False when there is none or the command is not a pane's, so
    /// the window runs it as usual.
    pub(crate) fn core_pane_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((tab, terminal)) = self.core_command_pane(window, cx) else {
            return false;
        };
        let Some(shown) = self.core_terminals.tabs.get_mut(&tab) else {
            return false;
        };
        // A mirror only watches: it moves focus and zooms, never splits.
        if shown.observe && matches!(command, Command::SplitRight | Command::SplitDown) {
            return true;
        }
        let direction = |command: Command| match command {
            Command::FocusLeft | Command::ResizeLeft | Command::SwapLeft => Some(Direction::Left),
            Command::FocusRight | Command::ResizeRight | Command::SwapRight => {
                Some(Direction::Right)
            }
            Command::FocusUp | Command::ResizeUp | Command::SwapUp => Some(Direction::Up),
            Command::FocusDown | Command::ResizeDown | Command::SwapDown => Some(Direction::Down),
            _ => None,
        };
        match command {
            Command::SplitRight | Command::SplitDown => {
                let axis = match command {
                    Command::SplitRight => Axis::Row,
                    _ => Axis::Column,
                };
                self.start_core_program(
                    Purpose::Split {
                        tab,
                        at: terminal,
                        axis,
                    },
                    sheprd_backend::Spawn::default(),
                    cx,
                );
            }
            Command::FocusLeft
            | Command::FocusRight
            | Command::FocusUp
            | Command::FocusDown
            | Command::NextPane
            | Command::PreviousPane
            | Command::LastPane => {
                let next = match command {
                    Command::NextPane | Command::PreviousPane => shown
                        .panes
                        .cycle(&terminal, command == Command::NextPane)
                        .filter(|next| *next != terminal),
                    // As Herdr's: the pane must still be there and not in use.
                    Command::LastPane => shown
                        .previous
                        .clone()
                        .filter(|last| *last != terminal && shown.panes.contains(last)),
                    _ => direction(command)
                        .and_then(|direction| shown.panes.neighbor(&terminal, direction)),
                };
                let Some(next) = next else {
                    return true;
                };
                // Moving on from a zoomed pane shows the split again.
                if shown.zoom.is_open() {
                    shown.set_zoom(None);
                }
                shown.activate(next);
                self.focus_core_terminal(tab, window, cx);
                cx.notify();
            }
            // The pane in use trades places with its neighbor and keeps the
            // keyboard, as Herdr's directional swaps do.
            Command::SwapLeft | Command::SwapRight | Command::SwapUp | Command::SwapDown => {
                let swapped = direction(command)
                    .and_then(|direction| shown.panes.neighbor(&terminal, direction))
                    .is_some_and(|other| shown.panes.swap(&terminal, &other));
                if swapped {
                    if shown.zoom.is_open() {
                        shown.set_zoom(None);
                    }
                    shown.unsaved = true;
                    cx.notify();
                }
            }
            Command::EqualizeSplits => {
                if shown.panes.equalize() {
                    shown.unsaved = true;
                    cx.notify();
                }
            }
            Command::ResizeLeft
            | Command::ResizeRight
            | Command::ResizeUp
            | Command::ResizeDown => {
                if direction(command)
                    .is_some_and(|direction| shown.panes.resize(&terminal, direction))
                {
                    shown.unsaved = true;
                    cx.notify();
                }
            }
            Command::Zoom => {
                if shown.panes.count() > 1 {
                    let zoom = (!shown.zoom.is_open()).then_some(terminal);
                    shown.set_zoom(zoom);
                    shown.unsaved = true;
                    cx.notify();
                }
            }
            Command::MirrorCorePane => self.mirror_core_pane(tab, terminal, window, cx),
            // The last pane closes as its tab does: the window's own close.
            Command::ClosePane if shown.panes.count() > 1 => {
                self.close_core_pane(tab, terminal, window, cx)
            }
            Command::RenameTab => self.rename_core(super::menu::Target::Tab(tab), window, cx),
            // A mirror's pane is named where it types.
            Command::RenamePane if !shown.observe => {
                self.rename_core(super::menu::Target::Pane { tab, terminal }, window, cx)
            }
            Command::MovePane if !shown.observe => self.open_core_move(tab, terminal, window, cx),
            // A mirror's pane is renamed and moved where it types, which it
            // says rather than reaching the Herdr pane behind it.
            Command::RenamePane | Command::MovePane => {
                self.show_flash(
                    Flash::warning(fl!("core-terminals-mirror-only-watches")),
                    cx,
                );
            }
            _ => return false,
        }
        true
    }

    /// Opens a watch-only view of `terminal` in a new tab of `tab`'s
    /// workspace, which a second group can show beside the first.
    pub(super) fn mirror_core_pane(
        &mut self,
        tab: TabId,
        terminal: TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((scope, workspace)) = cx
            .try_global::<Store>()
            .and_then(|store| store.get(tab))
            .map(|shown| (shown.scope.clone(), shown.workspace_id.clone()))
        else {
            return;
        };
        let location = Location::core_terminal(Panes::single(terminal), true);
        let Some(id) = Store::update(cx, |store| {
            store.open(scope, &workspace, Some(location), None)
        }) else {
            self.show_flash(Flash::warning(fl!("core-terminals-too-many-tabs")), cx);
            return;
        };
        self.sync_core_views(cx);
        self.show_browser_tab(id, window, cx);
    }

    /// Shows `terminal`, a terminal of this computer's core that something
    /// outside the window started, such as an automation's agent: the
    /// focused workspace's tab holding it, with it as the pane in use, else
    /// a new tab. A terminal a tab elsewhere drives opens watch-only here,
    /// so two tabs never type into it. Refused while another host is shown,
    /// whose workspaces cannot hold this computer's terminals.
    pub(crate) fn open_core_terminal(
        &mut self,
        terminal: TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_is_remote() {
            self.show_flash(Flash::warning(fl!("core-terminals-this-computer")), cx);
            return;
        }
        let Some((scope, workspace)) = self.browser_key() else {
            self.show_flash(Flash::warning(fl!("core-terminals-open-workspace")), cx);
            return;
        };
        let (shown, driven) = cx
            .try_global::<Store>()
            .map(|store| holding(store, &terminal, &scope, &workspace))
            .unwrap_or_default();
        if let Some(tab) = shown {
            self.sync_core_views(cx);
            self.show_core_pane(tab, &terminal, window, cx);
            return;
        }
        let location = Location::core_terminal(Panes::single(terminal), driven);
        let Some(id) = Store::update(cx, |store| {
            store.open(scope, &workspace, Some(location), None)
        }) else {
            self.show_flash(Flash::warning(fl!("core-terminals-too-many-tabs")), cx);
            return;
        };
        self.sync_core_views(cx);
        self.show_browser_tab(id, window, cx);
    }
}

/// The focused workspace's tab already showing `terminal`, driving or
/// watching, and whether any tab of this computer drives it. Every local
/// scope's tabs attach to the same core.
fn holding(
    store: &Store,
    terminal: &TerminalId,
    scope: &crate::browser::Scope,
    workspace: &str,
) -> (Option<TabId>, bool) {
    let mut shown = None;
    let mut driven = false;
    for tab in store.tabs().filter(|tab| tab.scope.is_local()) {
        let Some(Location::CoreTerminal { panes, observe, .. }) = &tab.location else {
            continue;
        };
        if !panes.contains(terminal) {
            continue;
        }
        driven |= !observe;
        if shown.is_none() && tab.scope == *scope && tab.workspace_id == workspace {
            shown = Some(tab.id);
        }
    }
    (shown, driven)
}

/// Whether `command` acts on the panes of the workspace in front: a kept
/// workspace has none of Herdr 1's, so with no herdr-core pane to take it,
/// it goes nowhere rather than to Herdr 1's focused pane behind. Closing is
/// left out: it closes the page a group shows instead.
pub(crate) fn acts_on_panes(command: Command) -> bool {
    matches!(
        command,
        Command::SplitRight
            | Command::SplitDown
            | Command::FocusLeft
            | Command::FocusRight
            | Command::FocusUp
            | Command::FocusDown
            | Command::NextPane
            | Command::PreviousPane
            | Command::LastPane
            | Command::SwapLeft
            | Command::SwapRight
            | Command::SwapUp
            | Command::SwapDown
            | Command::EqualizeSplits
            | Command::ResizeLeft
            | Command::ResizeRight
            | Command::ResizeUp
            | Command::ResizeDown
            | Command::Zoom
            | Command::MirrorCorePane
            | Command::RenameTab
            | Command::RenamePane
            | Command::MovePane
            | Command::Find
            | Command::FindNext
            | Command::FindPrevious
            | Command::ClearPane
            | Command::CopyMode
            | Command::EditScrollback
    )
}
