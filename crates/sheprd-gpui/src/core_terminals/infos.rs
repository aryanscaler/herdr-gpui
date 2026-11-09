//! What herdr-core reports about each terminal: its agent's status, its
//! folder, and its name, for the chrome of split panes. Read on a short-lived
//! thread every few seconds while a split is shown, never on the UI thread.

use sheprd_backend::{Backend as _, TerminalId, TerminalInfo};
use sheprd_core_client::CoreBackend;
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{Receiver, TryRecvError, sync_channel},
    time::{Duration, Instant},
};

/// How often the list is read again.
const EVERY: Duration = Duration::from_secs(2);

type Listed = Receiver<Result<Vec<TerminalInfo>, sheprd_core_client::Error>>;

#[derive(Default)]
pub(super) struct Infos {
    /// Terminals started since the last list was asked for, which a list
    /// already on its way may not name yet.
    fresh: HashSet<TerminalId>,
    known: HashMap<TerminalId, TerminalInfo>,
    last: Option<Instant>,
    /// When the list known was asked for.
    read: Option<Instant>,
    /// Lists read so far.
    lists: u64,
    /// The list on its way, and when it was asked for.
    pending: Option<(Instant, Listed)>,
}

impl Infos {
    pub(super) fn get(&self, id: &TerminalId) -> Option<&TerminalInfo> {
        self.known.get(id)
    }

    /// The terminal herdr-core knows by `name`.
    pub(super) fn by_name(&self, name: &str) -> Option<&TerminalInfo> {
        self.known
            .values()
            .find(|info| info.name.as_deref() == Some(name))
    }

    /// The terminal `id` names: its address, or else its name, as a terminal
    /// found by name is shown under it.
    pub(super) fn named(&self, id: &TerminalId) -> Option<&TerminalInfo> {
        self.get(id).or_else(|| {
            self.known
                .values()
                .find(|info| info.name.as_deref() == Some(id.as_str()))
        })
    }

    /// How many of the core's terminals run an agent that is working.
    pub(super) fn working(&self) -> u32 {
        let working = self
            .known
            .values()
            .filter(|info| info.agent == sheprd_client::protocol::AgentStatus::Working)
            .count();
        u32::try_from(working).unwrap_or(u32::MAX)
    }

    /// Every terminal the core last reported.
    pub(super) fn all(&self) -> impl Iterator<Item = &TerminalInfo> {
        self.known.values()
    }

    /// When the list known was asked for; none before the first.
    pub(super) fn read_at(&self) -> Option<Instant> {
        self.read
    }

    /// How many lists have been read; none means nothing is known yet.
    pub(super) fn lists(&self) -> u64 {
        self.lists
    }

    /// Takes `list` as a finished read would.
    #[cfg(test)]
    pub(super) fn list_for_test(&mut self, list: Vec<TerminalInfo>) {
        self.known = list
            .into_iter()
            .map(|info| (info.id.clone(), info))
            .collect();
        self.read = Some(Instant::now());
        self.lists += 1;
    }

    /// Records a terminal just started, until the next list replaces it,
    /// and hands it back.
    pub(super) fn insert(&mut self, info: TerminalInfo) -> TerminalInfo {
        self.fresh.insert(info.id.clone());
        self.known.insert(info.id.clone(), info.clone());
        info
    }

    /// Takes a finished read and starts the next when one is due. True when
    /// what is known changed.
    pub(super) fn poll(&mut self, backend: Option<&CoreBackend>, now: Instant) -> bool {
        let mut changed = false;
        if let Some((asked, pending)) = &self.pending {
            let asked = *asked;
            match pending.try_recv() {
                Ok(Ok(list)) => {
                    let mut known: HashMap<TerminalId, TerminalInfo> = list
                        .into_iter()
                        .map(|info| (info.id.clone(), info))
                        .collect();
                    // A fresh terminal outlives one list that missed it.
                    for id in self.fresh.drain() {
                        if let Some(info) = self.known.get(&id)
                            && !known.contains_key(&id)
                        {
                            known.insert(id, info.clone());
                        }
                    }
                    changed = known != self.known;
                    self.known = known;
                    self.read = Some(asked);
                    self.lists += 1;
                    self.pending = None;
                }
                Ok(Err(error)) => {
                    tracing::debug!(%error, "Could not list herdr-core terminals");
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => self.pending = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let due = self
            .last
            .is_none_or(|last| now.duration_since(last) >= EVERY);
        if let Some(backend) = backend.filter(|_| self.pending.is_none() && due) {
            self.last = Some(now);
            let backend = backend.clone();
            let (send, receive) = sync_channel(1);
            let started = std::thread::Builder::new()
                .name("herdr-core-list".into())
                .spawn(move || {
                    let _ = send.send(backend.terminals());
                });
            match started {
                Ok(_) => self.pending = Some((now, receive)),
                Err(error) => tracing::warn!(%error, "Could not start a herdr-core list worker"),
            }
        }
        changed
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
