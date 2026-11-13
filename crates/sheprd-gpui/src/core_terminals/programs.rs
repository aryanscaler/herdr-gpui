//! Programs the app runs in tabs of the workspaces it keeps, such as a
//! worktree script, and how they end. herdr-core reports a terminal's labels
//! as soon as it starts, and its tab's view how its program ended.

use super::{CoreTerminalView, Host, Purpose, workspaces::scope_of};
use crate::{
    SheprdWindow,
    browser::{Location, Store, TabId},
};
use gpui::{App, Context, Entity, Window};
use sheprd_backend::{Exit, Spawn, TerminalId};

impl SheprdWindow {
    /// Starts `spawn` in a new tab of the kept workspace `id` on the shown
    /// host, which the tab is brought to once it opens.
    pub(crate) fn start_in_kept_workspace(
        &mut self,
        id: &str,
        spawn: Spawn,
        cx: &mut Context<Self>,
    ) -> crate::Result<()> {
        self.try_start_in_kept_workspace(id, spawn, cx).map(drop)
    }

    /// Starts `spawn` as [`Self::start_in_kept_workspace`] does, telling
    /// whether herdr-core's start queue took it, so a caller that must not
    /// lose it can try again.
    pub(crate) fn try_start_in_kept_workspace(
        &mut self,
        id: &str,
        spawn: Spawn,
        cx: &mut Context<Self>,
    ) -> crate::Result<bool> {
        if !self.focus_kept_workspace(id, cx) {
            return Err(crate::Error::StaleWorkspace);
        }
        let purpose = Purpose::Tab {
            scope: scope_of(self),
            workspace: id.to_owned(),
        };
        Ok(self.start_core_program(purpose, spawn, cx))
    }

    pub(crate) fn host_terminal_labelled(
        &self,
        host: &Host,
        key: &str,
        value: &str,
    ) -> Option<TerminalId> {
        self.core_terminals
            .infos_of(host)?
            .all()
            .find(|info| info.labels.get(key).is_some_and(|known| known == value))
            .map(|info| info.id.clone())
    }

    /// How this computer's `terminal` ended, once a tab showing it saw it
    /// end. Terminal IDs repeat from one core to another, so a tab of
    /// another host's core never answers for it.
    #[cfg(test)]
    pub(crate) fn core_terminal_exit(&self, terminal: &TerminalId, cx: &App) -> Option<Exit> {
        self.host_terminal_exit(&Host::Local, terminal, cx)
    }

    pub(crate) fn host_terminal_exit(
        &self,
        host: &Host,
        terminal: &TerminalId,
        cx: &App,
    ) -> Option<Exit> {
        self.host_views(host, terminal)
            .find_map(|(_, view)| view.read(cx).exit())
    }

    /// Whether a tab shows this computer's `terminal`.
    #[cfg(test)]
    pub(crate) fn local_terminal_shown(&self, terminal: &TerminalId) -> bool {
        self.host_terminal_shown(&Host::Local, terminal)
    }

    pub(crate) fn host_terminal_shown(&self, host: &Host, terminal: &TerminalId) -> bool {
        self.host_views(host, terminal).next().is_some()
    }

    fn host_views<'a>(
        &'a self,
        host: &'a Host,
        terminal: &'a TerminalId,
    ) -> impl Iterator<Item = (TabId, &'a Entity<CoreTerminalView>)> + 'a {
        self.core_terminals
            .views
            .iter()
            .filter(move |((tab, shown), _)| {
                shown == terminal && &self.core_terminals.host_of(*tab) == host
            })
            .map(|((tab, _), view)| (*tab, view))
    }

    /// Brings the kept workspace `id` of this computer to the front on the
    /// tab showing `terminal`, opening one in it when none does, as when a
    /// workspace was kept at a run's checkout before its agent started.
    /// Whether a tab shows it now.
    pub(crate) fn show_terminal_in_kept_workspace(
        &mut self,
        id: &str,
        terminal: TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let scope = scope_of(self);
        let showing = cx.try_global::<Store>().and_then(|store| {
            store
                .in_workspace(&scope, id)
                .find(|tab| match &tab.location {
                    Some(Location::CoreTerminal { panes, .. }) => {
                        panes.terminals().contains(&&terminal)
                    }
                    _ => false,
                })
                .map(|tab| tab.id)
        });
        self.focus_kept_workspace(id, cx);
        let tab = match showing {
            Some(tab) => tab,
            None => {
                let location =
                    Location::core_terminal(super::panes::Panes::single(terminal), false);
                let opened = Store::update(cx, |store| store.open(scope, id, Some(location), None));
                let Some(tab) = opened else {
                    self.show_flash(
                        crate::window::Flash::warning(crate::i18n::fl!(
                            "core-terminals-too-many-tabs"
                        )),
                        cx,
                    );
                    return false;
                };
                self.sync_core_views(cx);
                tab
            }
        };
        self.show_browser_tab(tab, window, cx);
        true
    }
}
