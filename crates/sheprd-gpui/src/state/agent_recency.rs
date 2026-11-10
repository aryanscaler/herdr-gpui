//! When this client saw each agent last change state, on one clock every
//! connection shares. A daemon's `state_change_seq` counts only its own
//! changes, so an agents panel that merges several hosts by recency needs an
//! order across them, as Herdr's terminal client keeps one.

use sheprd_client::protocol::{AgentStatus, ClientShellSnapshot};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

/// Shared by every connection's reader thread, so changes on different hosts
/// order as they arrived here.
static CLOCK: AtomicU64 = AtomicU64::new(0);

/// Each listed agent's latest observed change, by pane; larger is newer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentRecency(HashMap<String, u64>);

impl AgentRecency {
    /// Stamps the agents whose state changed since `previous`, the last
    /// snapshot from the same daemon boot, oldest change first, and forgets
    /// panes `next` no longer lists. Without one, every agent is new.
    pub(super) fn observe(
        &mut self,
        previous: Option<&ClientShellSnapshot>,
        next: &ClientShellSnapshot,
    ) {
        let mut changed: Vec<_> = next
            .agents
            .iter()
            .filter(|agent| {
                previous
                    .and_then(|previous| {
                        previous
                            .agents
                            .iter()
                            .find(|old| old.pane_id == agent.pane_id)
                    })
                    .is_none_or(|old| old.state_change_seq != agent.state_change_seq)
            })
            .collect();
        changed.sort_by_key(|agent| agent.state_change_seq);
        for agent in changed {
            let stamp = CLOCK.fetch_add(1, Ordering::Relaxed) + 1;
            self.0.insert(agent.pane_id.clone(), stamp);
        }
        self.0
            .retain(|pane_id, _| next.agents.iter().any(|agent| agent.pane_id == *pane_id));
    }

    /// The pane's latest observed change, or 0 when none was.
    pub(crate) fn of(&self, pane_id: &str) -> u64 {
        self.0.get(pane_id).copied().unwrap_or_default()
    }
}

/// When this client saw each herdr-core agent last change state, on the
/// same clock as [`AgentRecency`], so the agents panel orders Herdr 2
/// agents among Herdr 1 ones. The core counts no changes, so a change is a
/// status that differs from the one last observed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatusRecency(HashMap<String, (AgentStatus, u64)>);

impl StatusRecency {
    /// Stamps the agents of `listed`, by terminal, whose status changed, and
    /// forgets those no longer listed. An agent seen for the first time is
    /// new.
    pub(crate) fn observe<'a>(&mut self, listed: impl IntoIterator<Item = (&'a str, AgentStatus)>) {
        let mut kept = HashMap::new();
        for (terminal, status) in listed {
            let stamp = match self.0.get(terminal) {
                Some((before, stamp)) if *before == status => *stamp,
                _ => CLOCK.fetch_add(1, Ordering::Relaxed) + 1,
            };
            kept.insert(terminal.to_owned(), (status, stamp));
        }
        self.0 = kept;
    }

    /// The terminal's latest observed change, or 0 when none was.
    pub(crate) fn of(&self, terminal: &str) -> u64 {
        self.0.get(terminal).map_or(0, |(_, stamp)| *stamp)
    }
}
