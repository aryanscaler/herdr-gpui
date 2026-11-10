//! The process list of a herdr-core pane, as a Herdr pane's menu lists its
//! own: the same pages and worker (`pane_menu::PaneProcesses`), asked of
//! this computer's core. Another host's terminal reports process IDs of
//! that host, which sampled here would name unrelated local processes, so
//! only this computer's panes offer it.

use super::{Host, menu::CoreMenu, menu::Target};
use crate::{
    SheprdWindow,
    browser::TabId,
    pane_menu::PaneProcesses,
    processes::{Source, Watch},
    window::Flash,
};
use gpui::{App, Context};
use sheprd_backend::TerminalId;

impl CoreMenu {
    pub(crate) fn processes(&self) -> Option<&PaneProcesses> {
        self.processes.as_ref()
    }

    pub(crate) fn processes_mut(&mut self) -> Option<&mut PaneProcesses> {
        self.processes.as_mut()
    }

    pub(crate) fn set_processes(&mut self, processes: PaneProcesses) {
        self.processes = Some(processes);
    }
}

impl SheprdWindow {
    /// Where the processes of `terminal`, a pane of `tab`, are asked: this
    /// computer's core, for a pane it runs. None for another host's.
    pub(crate) fn core_processes_source(
        &self,
        tab: TabId,
        terminal: &TerminalId,
    ) -> Option<Source> {
        let shown = self.core_terminals.tabs.get(&tab)?;
        (shown.host == Host::Local && shown.panes.contains(terminal)).then(|| Source::AppCore {
            terminal: terminal.as_str().to_owned(),
        })
    }

    /// Opens the process list of `terminal` in `tab` on the menu's page.
    pub(crate) fn open_core_processes(
        &mut self,
        tab: TabId,
        terminal: &TerminalId,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.core_processes_source(tab, terminal) else {
            return;
        };
        match Watch::start(source) {
            Ok(watch) => self.show_core_processes(watch, cx),
            Err(error) => self.show_flash(Flash::warning(error.to_string()), cx),
        }
    }

    /// Whether the pane whose processes the menu lists is still its tab's.
    pub(crate) fn core_processes_current(&self) -> crate::Result<()> {
        match self.menu.core.as_ref().map(CoreMenu::target) {
            Some(Target::Pane { tab, terminal })
                if self
                    .core_terminals
                    .tabs
                    .get(tab)
                    .is_some_and(|shown| shown.panes.contains(terminal)) =>
            {
                Ok(())
            }
            _ => Err(crate::Error::StalePane),
        }
    }

    /// The name of the pane the menu targets, as its header shows it.
    pub(crate) fn core_menu_pane_label(&self, cx: &App) -> String {
        match self.menu.core.as_ref().map(CoreMenu::target) {
            Some(Target::Pane { tab, terminal }) => self.core_pane_label(*tab, terminal, cx).0,
            _ => String::new(),
        }
    }
}
