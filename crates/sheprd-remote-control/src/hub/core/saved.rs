//! Which of Sheprd's saved tabs a Herdr 2 pane is in. While Sheprd runs,
//! only it changes such a tab's layout (`control::layout`): a phone names
//! the tab and the pane's terminal, and Sheprd splits or zooms it as the
//! desktop would; while it is closed, the daemon does (`super::arrange`).

use super::{Source, lock, pane_id};
use crate::hub::Hub;

/// How a saved tab's id reads in the layout.
pub(super) const SAVED_TAB: &str = "core:tab:";

impl Source {
    /// The id of the saved tab `tab` names, and the address of `pane`'s
    /// terminal; `None` when either is not this core's.
    pub(super) fn saved_target(&self, tab: &str, pane: &str) -> Option<(u64, String)> {
        let id = tab.strip_prefix(SAVED_TAB)?.parse().ok()?;
        let state = lock(&self.state);
        let boot = &state.core.as_ref()?.hello().core.id;
        let terminal = state
            .terminals
            .iter()
            .find(|terminal| pane_id(boot, &terminal.address) == pane)?;
        Some((id, terminal.address.clone()))
    }

    /// Whether Sheprd keeps workspace `id` for this computer's Herdr 2.
    pub(super) fn keeps_workspace(&self, id: &str) -> bool {
        self.scope
            .as_deref()
            .is_some_and(|scope| lock(&self.poller).keeps(scope, id))
    }

    /// The pane id of this core's terminal at `address`.
    fn pane_of(&self, address: &str) -> Option<String> {
        let state = lock(&self.state);
        Some(pane_id(&state.core.as_ref()?.hello().core.id, address))
    }
}

impl Hub {
    /// Whether `workspace` is one Sheprd keeps for Herdr 2, which only
    /// Sheprd renames.
    pub(crate) fn kept_workspace(&self, workspace: &str) -> bool {
        self.core
            .as_ref()
            .is_some_and(|core| core.keeps_workspace(workspace))
    }

    /// The pane of the core terminal at `address`, such as a split Sheprd
    /// made for a phone, once listed so the phone can open it; after a short
    /// wait it is answered anyway, and the phone finds it on reload.
    pub(crate) async fn core_pane_listed(&self, address: &str) -> Option<String> {
        let pane = self.core.as_ref()?.pane_of(address)?;
        let mut layout = self.layout.clone();
        let listed = layout.wait_for(|layout| layout.terminal(&pane).is_some());
        let _ = tokio::time::timeout(crate::hub::CORE_LISTING, listed).await;
        Some(pane)
    }
}
