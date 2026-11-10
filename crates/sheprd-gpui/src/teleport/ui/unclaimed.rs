//! What a finished move still asks of the app: closing the source's Herdr 2
//! tabs and opening the ones rebuilt on the destination. The window that
//! started it does this; when that window closed first, the move leaves it
//! here for whichever window ticks next, since the app, not a daemon, keeps
//! Herdr 2 tabs.

use super::{Error, Event, Outcome};
use crate::{
    SheprdWindow,
    browser::{Location, Scope, Store, TabId},
    core_terminals::{Host, Purpose},
    i18n::fl,
    teleport::{Keeping, Landed, Source},
    window::Flash,
};
use gpui::{Context, Global, Window};
use sheprd_backend::Spawn;
use sheprd_session::panes::Panes;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, mpsc},
};

/// What the app still has to do once a move finishes, as the window that
/// started it names it: another window may name a different session
/// `local`, so its scopes are read once, there.
#[derive(Clone)]
pub(super) struct Handover {
    /// Where the destination's tabs belong.
    pub(super) scope: Scope,
    /// The source's Herdr 2 tabs and the panes that moved from each.
    pub(super) closing: Vec<(TabId, Panes)>,
    /// The workspace moved, which keeps one idle shell when the app keeps it.
    pub(super) source: Source,
    /// Where the source's workspace and that shell belong.
    pub(super) source_scope: Scope,
}

/// A move that finished after its window closed.
pub(super) struct Leftover {
    pub(super) outcome: Outcome,
    pub(super) handover: Handover,
}

/// Moves that finished with no window to take them, shared with the
/// workers that run them.
#[derive(Clone, Default)]
pub(super) struct Unclaimed(Arc<Mutex<Vec<Leftover>>>);

impl Global for Unclaimed {}

impl Unclaimed {
    pub(super) fn push(&self, leftover: Leftover) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(leftover);
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    fn take(&self) -> Vec<Leftover> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Hands a move's `result` to its window, or, when that window closed, a
/// finished move's leftover work to `unclaimed`.
pub(super) fn deliver(
    sender: &mpsc::Sender<Event>,
    result: Result<Outcome, Error>,
    handover: Handover,
    unclaimed: &Unclaimed,
) {
    if let Err(mpsc::SendError(Event::Moved(Ok(outcome)))) = sender.send(Event::Moved(result)) {
        unclaimed.push(Leftover { outcome, handover });
    }
}

/// Of the source's Herdr 2 tabs, those whose every terminal moved. `now`
/// gives a tab's panes today. The window drops ended panes by itself, so a
/// tab with fewer panes still closes; one split during the move holds a
/// terminal that did not move and stays open with it.
pub(super) fn moved_tabs(
    closing: Vec<(TabId, Panes)>,
    now: impl Fn(TabId) -> Option<Panes>,
) -> Vec<TabId> {
    closing
        .into_iter()
        .filter(|(tab, moved)| {
            now(*tab).is_some_and(|panes| {
                panes
                    .terminals()
                    .into_iter()
                    .all(|terminal| moved.contains(terminal))
            })
        })
        .map(|(tab, _)| tab)
        .collect()
}

impl SheprdWindow {
    /// Finishes the moves whose window closed before they did.
    pub(super) fn claim_teleports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(unclaimed) = cx.try_global::<Unclaimed>().cloned() else {
            return;
        };
        for Leftover { outcome, handover } in unclaimed.take() {
            let store = cx.try_global::<Store>();
            let panes = |tab| match store?.get(tab)?.location.as_ref()? {
                Location::CoreTerminal { panes, .. } => Some(panes.clone()),
                _ => None,
            };
            for tab in moved_tabs(handover.closing, panes) {
                self.close_browser_tab(tab, window, cx);
            }
            self.settle_teleport(&outcome, &handover.scope, window, cx);
            // Only a finished move is left here: its programs stopped.
            self.retire_kept_source(&handover.source, handover.source_scope, cx);
        }
    }

    /// The scope of this window's endpoint `endpoint_id`, where its
    /// workspaces' tabs belong.
    pub(super) fn endpoint_scope(&self, endpoint_id: &str) -> Scope {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.id == endpoint_id)
            .map_or_else(|| Scope::endpoint(endpoint_id), crate::browser::scope)
    }

    /// Settles where a move landed: keeps the new workspace the app keeps
    /// there, closes the tabs of a kept checkout the work came back to, and
    /// opens the tabs rebuilt in the destination's Herdr 2. Their terminals
    /// already run there; the window joins them as it shows the workspace.
    /// Answers the workspace, or `None` when it could not be kept, which a
    /// flash says; then the terminals end, since no tab would show them.
    pub(super) fn settle_teleport(
        &mut self,
        outcome: &Outcome,
        scope: &Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let workspace = match &outcome.workspace {
            Landed::Workspace(id) => {
                // A kept checkout's own tabs, like its idle teleported shell,
                // give way to the work's, as Herdr 1's do in the move.
                let stale: Vec<TabId> = cx
                    .try_global::<Store>()
                    .filter(|store| store.kept_workspace(scope, id).is_some())
                    .map(|store| store.in_workspace(scope, id).map(|tab| tab.id).collect())
                    .unwrap_or_default();
                for tab in stale {
                    self.close_browser_tab(tab, window, cx);
                }
                id.clone()
            }
            Landed::New { checkout, label } => {
                let (label, custom) =
                    crate::core_terminals::kept_label(label.as_deref(), Some(Path::new(checkout)));
                let kept = Store::update(cx, |store| {
                    store.keep_workspace(scope, &label, checkout, custom)
                });
                let Some(id) = kept else {
                    let host = Host::of(scope);
                    let ending = outcome
                        .core_tabs
                        .iter()
                        .flat_map(Panes::terminals)
                        .map(|terminal| (host.clone(), terminal.clone()))
                        .collect::<Vec<_>>();
                    Store::update(cx, |store| store.end_terminals(ending));
                    self.show_flash(Flash::warning(fl!("core-workspaces-too-many")), cx);
                    return None;
                };
                id
            }
        };
        if !outcome.core_tabs.is_empty() {
            // Checked before the move, so only tabs opened meanwhile can
            // leave one out; the rest still open.
            let missed = Store::update(cx, |store| {
                outcome
                    .core_tabs
                    .iter()
                    .filter(|panes| {
                        let location = Location::core_terminal((*panes).clone(), false);
                        store
                            .open(scope.clone(), &workspace, Some(location), None)
                            .is_none()
                    })
                    .count()
            });
            if missed > 0 {
                self.show_flash(Flash::warning(fl!("core-terminals-too-many-tabs")), cx);
            }
        }
        self.refresh_core_session(cx);
        cx.notify();
        Some(workspace)
    }

    /// Leaves one idle shell in a kept source workspace whose programs the
    /// move stopped, as the move does in a Herdr 1 one.
    pub(super) fn retire_kept_source(
        &mut self,
        source: &Source,
        scope: Scope,
        cx: &mut Context<Self>,
    ) {
        let Keeping::App { checkout } = &source.keeping else {
            return;
        };
        let spawn = Spawn {
            cwd: Some(PathBuf::from(checkout)),
            ..Spawn::default()
        };
        let workspace = source.workspace_id.clone();
        self.start_core_program(Purpose::Tab { scope, workspace }, spawn, cx);
    }
}
