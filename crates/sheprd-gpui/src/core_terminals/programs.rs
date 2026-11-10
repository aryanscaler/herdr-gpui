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
        if !self.focus_kept_workspace(id, cx) {
            return Err(crate::Error::StaleWorkspace);
        }
        let purpose = Purpose::Tab {
            scope: scope_of(self),
            workspace: id.to_owned(),
        };
        self.start_core_program(purpose, spawn, cx);
        Ok(())
    }

    /// The terminal of this computer's core that `key` labels `value`.
    pub(crate) fn local_terminal_labelled(&self, key: &str, value: &str) -> Option<TerminalId> {
        self.core_terminals
            .infos
            .all()
            .find(|info| info.labels.get(key).is_some_and(|known| known == value))
            .map(|info| info.id.clone())
    }

    /// How this computer's `terminal` ended, once a tab showing it saw it
    /// end. Terminal IDs repeat from one core to another, so a tab of
    /// another host's core never answers for it.
    pub(crate) fn core_terminal_exit(&self, terminal: &TerminalId, cx: &App) -> Option<Exit> {
        self.local_views(terminal)
            .find_map(|(_, view)| view.read(cx).exit())
    }

    /// Whether a tab shows this computer's `terminal`.
    pub(crate) fn local_terminal_shown(&self, terminal: &TerminalId) -> bool {
        self.local_views(terminal).next().is_some()
    }

    fn local_views<'a>(
        &'a self,
        terminal: &'a TerminalId,
    ) -> impl Iterator<Item = (TabId, &'a Entity<CoreTerminalView>)> + 'a {
        self.core_terminals
            .views
            .iter()
            .filter(move |((tab, shown), _)| {
                shown == terminal && self.core_terminals.host_of(*tab) == Host::Local
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
