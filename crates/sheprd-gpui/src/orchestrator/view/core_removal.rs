//! Closing the workspace the app keeps for a Herdr 2 run once the run is
//! removed. The worker removes the checkout and forgets the run, which its
//! next snapshot shows; the window then closes the kept workspace at the
//! run's checkout, as Herdr 1 closes a removed worktree's workspace.

use super::{Event, OrchestratorView, rows::Runs};
use gpui::Context;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Checkout {
    pub target: sheprd_client::ConnectTarget,
    pub path: PathBuf,
}

/// Removals followed at once; the oldest is let go beyond this.
const MAX_FOLLOWED: usize = 32;

/// The host and checkout of run `run` when Herdr 2 runs it.
pub(super) fn core_checkout(runs: &Runs<'_>, run: usize) -> Option<Checkout> {
    let session = runs.session(run)?;
    if !matches!(session.pane, crate::orchestrator::Pane::Herdr2 { .. }) {
        return None;
    }
    Some(Checkout {
        target: crate::orchestrator::actions::target_of(session),
        path: runs.runs.get(run)?.workspace.as_ref()?.path.clone()?,
    })
}

/// The checkouts of followed removals whose run no snapshot lists any more,
/// taken out of `followed`; the others stay followed.
pub(super) fn removed(
    followed: &mut Vec<(String, Checkout)>,
    listed: impl Fn(&str) -> bool,
) -> Vec<Checkout> {
    let mut gone = Vec::new();
    followed.retain(|(run, path)| {
        if listed(run) {
            return true;
        }
        gone.push(path.clone());
        false
    });
    gone
}

impl OrchestratorView {
    /// Follows the removal of run `run`, by ID, when Herdr 2 runs it.
    pub(super) fn follow_core_removal(&mut self, run: &str) {
        let runs = Runs::new(&self.snapshot().runs, &self.snapshot().sessions, &self.live);
        let checkout = runs
            .runs
            .iter()
            .position(|known| known.id == run)
            .and_then(|index| core_checkout(&runs, index));
        let Some(checkout) = checkout else {
            return;
        };
        if self.core_removals.len() >= MAX_FOLLOWED {
            self.core_removals.remove(0);
        }
        self.core_removals.push((run.to_owned(), checkout));
    }

    /// Asks the window to close the kept workspace of each followed run the
    /// worker has removed.
    pub(super) fn close_removed_core_runs(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = &self.pending_core_open {
            let runs = Runs::new(&self.snapshot().runs, &self.snapshot().sessions, &self.live);
            if let Some(index) = runs.runs.iter().position(|run| &run.id == id)
                && let Some(event) = super::core_run(&runs, index)
            {
                cx.emit(event);
                self.pending_core_open = None;
            }
        }
        let repos = &self.repos;
        let listed = |run: &str| {
            repos
                .iter()
                .any(|repo| repo.snapshot.runs.iter().any(|known| known.id == run))
        };
        for Checkout { target, path } in removed(&mut self.core_removals, listed) {
            cx.emit(Event::CloseCoreRun { target, path });
        }
    }
}
