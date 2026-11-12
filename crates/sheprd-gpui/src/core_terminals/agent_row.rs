//! The agents-list seam on herdr-core panes: whether a core terminal is the
//! pane on screen, for row actions that need that reading.

use super::SheprdWindow;
use crate::browser::TabId;
use sheprd_backend::TerminalId;

impl SheprdWindow {
    /// Whether `terminal` is the pane in use of the tab on screen: the
    /// client-local reading of "this row's pane is already shown", which
    /// `focused_pane` refines with the keyboard holder where a site wants
    /// it. A chat drawn over the pane holds the keyboard while the pane is
    /// still the shown one, so pane-in-use is the default reading.
    // Callers come with the devices overview's agent rows adopting the
    // resolver.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn core_pane_shown(&self, tab: TabId, terminal: &TerminalId) -> bool {
        self.core_terminals
            .shown
            .as_ref()
            .is_some_and(|(shown_tab, shown_terminal)| {
                *shown_tab == tab && shown_terminal == terminal
            })
    }
}
