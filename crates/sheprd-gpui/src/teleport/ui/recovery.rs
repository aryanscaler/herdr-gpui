//! Recover committed destination tabs after process loss. Disk work always
//! runs on a worker; retries are bounded and never launch or stop a terminal.

use super::*;
use sheprd_backend::TerminalId;
use sheprd_remote::teleport::arrival::{self, Receipt};
use sheprd_session::panes::Panes;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{LazyLock, Mutex, PoisonError},
};

const RETRY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Recovery {
    pending: Arc<Mutex<Vec<(PathBuf, Receipt)>>>,
    scanning: Arc<AtomicBool>,
    next: Option<Instant>,
}

impl Global for Recovery {}

/// A receipt this run already turned into tabs, by a finished move or by
/// recovery. Its tabs are never opened again: closing one is the user's
/// choice, not process loss.
struct Adopted {
    scope: Scope,
    receipt: Receipt,
    /// The terminals its tabs hold.
    terminals: Vec<TerminalId>,
    /// Of those, the ones whose tabs have been open since it was adopted.
    /// Until a tab opens, its absence is a move still landing, not a close.
    seen: HashSet<TerminalId>,
    /// Of those, the ones whose tabs the user has closed again.
    closed: Vec<TerminalId>,
    /// The most tabs and workspaces the app held while some of its tabs had
    /// never opened. A tab or workspace limit can leave them out; they are
    /// tried again once the user frees room, not on every scan.
    room: Room,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Room {
    tabs: usize,
    workspaces: usize,
}

impl Room {
    fn of(store: &Store) -> Self {
        Self {
            tabs: store.tabs().count(),
            workspaces: store.kept_ids().count(),
        }
    }

    fn freed_since(self, before: Self) -> bool {
        self.tabs < before.tabs || self.workspaces < before.workspaces
    }

    fn max(self, other: Self) -> Self {
        Self {
            tabs: self.tabs.max(other.tabs),
            workspaces: self.workspaces.max(other.workspaces),
        }
    }
}

/// Process-wide, as both the move worker (`persist`) and every window's
/// recovery record adoptions. Entries leave when their receipt is released.
static ADOPTED: LazyLock<Mutex<HashMap<PathBuf, Adopted>>> = LazyLock::new(Mutex::default);

fn adopted() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Adopted>> {
    ADOPTED.lock().unwrap_or_else(PoisonError::into_inner)
}

fn adopt(path: PathBuf, scope: Scope, receipt: Receipt) {
    let terminals = receipt
        .outcome()
        .core_tabs
        .iter()
        .flat_map(Panes::terminals)
        .cloned()
        .collect();
    adopted().entry(path).or_insert(Adopted {
        scope,
        receipt,
        terminals,
        seen: HashSet::new(),
        closed: Vec::new(),
        room: Room::default(),
    });
}

fn directory() -> Option<PathBuf> {
    Store::path().and_then(|path| Some(path.parent()?.join("teleport-arrivals")))
}

/// Synchronous by design: this is called on the teleport worker at the commit
/// point, before it may retire the source. A failed durable write fails the move.
pub(super) fn persist(
    scope: &Scope,
    outcome: &Outcome,
) -> sheprd_remote::teleport::error::Result<()> {
    if outcome.core_tabs.is_empty() {
        return Ok(());
    }
    let directory = directory().ok_or(arrival::Error::Unavailable)?;
    let receipt = Receipt::new(scope.as_str(), outcome);
    let path = arrival::save(&directory, &receipt)?;
    // The finished move opens these tabs itself; recovery must not reopen
    // them after the user closes one.
    adopt(path, scope.clone(), receipt);
    Ok(())
}

impl Recovery {
    fn scan(&mut self) {
        // Headless tests must never read the user's persisted arrivals.
        if cfg!(test) {
            return;
        }
        let now = Instant::now();
        if self.scanning.load(Ordering::Acquire) || self.next.is_some_and(|next| now < next) {
            return;
        }
        let (Some(directory), Some(saved)) = (directory(), Store::path()) else {
            return;
        };
        self.next = Some(now + RETRY);
        self.scanning.store(true, Ordering::Release);
        let scanning = self.scanning.clone();
        let pending = self.pending.clone();
        let started = std::thread::Builder::new().name("teleport-arrivals".into()).spawn(move || {
            match arrival::load(&directory) {
                Ok(receipts) => {
                    let receipts = receipts.into_iter().filter(|(path, receipt)| {
                        let closed = adopted().get(path).map(|adopted| adopted.closed.clone()).unwrap_or_default();
                        match arrival::release(path, receipt, &saved, &closed) {
                            Ok(true) => {
                                adopted().remove(path);
                                false
                            }
                            Ok(false) => true,
                            Err(error) => { tracing::warn!(%error, "Could not release a saved Teleport arrival"); true }
                        }
                    }).collect();
                    *pending.lock().unwrap_or_else(PoisonError::into_inner) = receipts;
                }
                Err(error) => tracing::warn!(%error, "Could not read pending Teleport arrivals"),
            }
            scanning.store(false, Ordering::Release);
        });
        if let Err(error) = started {
            self.scanning.store(false, Ordering::Release);
            tracing::warn!(%error, "Could not start Teleport arrival recovery");
        }
    }
}

impl SheprdWindow {
    pub(super) fn recover_teleport_arrivals(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let recovery = cx.default_global::<Recovery>();
        recovery.scan();
        let pending = std::mem::take(
            &mut *recovery
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for (path, receipt) in pending {
            if adopted().contains_key(&path) {
                continue;
            }
            let scope = Scope::endpoint(receipt.scope());
            let outcome = receipt.outcome();
            adopt(path, scope.clone(), receipt);
            self.settle_teleport(&outcome, &scope, window, cx);
        }
        // Settled outside the lock: settling reads the store, never adoptions.
        for (scope, outcome) in track(cx) {
            self.settle_teleport(&outcome, &scope, window, cx);
        }
    }
}

/// Note which adopted terminals have tabs open now, and which were open and
/// have since been closed, so their receipts stop waiting for them. Returns
/// the never-opened tabs to try again now that the user has freed room.
fn track(cx: &App) -> Vec<(Scope, Outcome)> {
    let mut adopted = adopted();
    if adopted.is_empty() {
        return Vec::new();
    }
    let Some(store) = cx.try_global::<Store>() else {
        return Vec::new();
    };
    let room = Room::of(store);
    let mut retries = Vec::new();
    for entry in adopted.values_mut() {
        let held: HashSet<&TerminalId> = store
            .in_scope(&entry.scope)
            .filter_map(|tab| match &tab.location {
                Some(Location::CoreTerminal { panes, .. }) => Some(panes.terminals()),
                _ => None,
            })
            .flatten()
            .collect();
        entry.seen.extend(
            entry
                .terminals
                .iter()
                .filter(|terminal| held.contains(terminal))
                .cloned(),
        );
        entry.closed = entry
            .seen
            .iter()
            .filter(|terminal| !held.contains(terminal))
            .cloned()
            .collect();
        // Only tabs none of whose terminals ever opened: one the user closed
        // stays closed.
        let mut outcome = entry.receipt.outcome();
        outcome.core_tabs.retain(|panes| {
            panes
                .terminals()
                .iter()
                .all(|terminal| !entry.seen.contains(*terminal))
        });
        if outcome.core_tabs.is_empty() {
            continue;
        }
        if room.freed_since(entry.room) {
            entry.room = room;
            retries.push((entry.scope.clone(), outcome));
        } else {
            entry.room = entry.room.max(room);
        }
    }
    retries
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
