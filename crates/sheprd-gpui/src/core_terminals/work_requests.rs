//! Work's independent request namespace. A new connection suppresses its
//! first snapshot; unseen terminals and read-only mirrors never gain authority.
use super::{CoreTerminals, Host, requests::LabelRequests};
use sheprd_backend::TerminalId;
use sheprd_control_protocol::protocol::work::remote::request_id;
use std::{collections::HashSet, time::Instant};

const MAX_SEEN: usize = 512;

#[derive(Default)]
pub(super) struct Seen {
    backend: Option<sheprd_core_client::CoreBackend>,
    connected_at: Option<Instant>,
    known: Option<HashSet<(TerminalId, String)>>,
    saturated: bool,
}

impl Seen {
    fn pause(&mut self, now: Instant) {
        self.known = None;
        self.connected_at = Some(now);
    }

    fn fresh(&self, read: Option<Instant>) -> bool {
        read.zip(self.connected_at)
            .is_some_and(|(read, connected)| read > connected)
    }

    fn take(&mut self, listed: HashSet<(TerminalId, String)>) -> HashSet<(TerminalId, String)> {
        if self.saturated || listed.len() > MAX_SEEN {
            // Do not forget an unrecorded request and later replay it when
            // capacity returns. A new connection starts with a fresh baseline.
            self.saturated = true;
            return HashSet::new();
        }
        let new = self
            .known
            .as_ref()
            .map(|known| listed.difference(known).cloned().collect())
            .unwrap_or_default();
        self.known = Some(listed);
        new
    }
}

impl CoreTerminals {
    pub(crate) fn drives_work_terminal(
        &self,
        scope: &crate::browser::Scope,
        terminal: &TerminalId,
    ) -> bool {
        let host = Host::Remote(scope.clone());
        self.tabs
            .values()
            .any(|shown| shown.host == host && !shown.observe && shown.panes.contains(terminal))
    }

    pub(crate) fn work_label_requests(&mut self) -> Vec<LabelRequests> {
        let mut found = Vec::new();
        for (scope, remote) in &mut self.remote {
            let Some(backend) = remote
                .hub
                .backend()
                .filter(|backend| !backend.core().is_lost())
            else {
                remote.work_requests = Seen::default();
                continue;
            };
            if remote
                .work_requests
                .backend
                .as_ref()
                .is_none_or(|known| !std::ptr::eq(known.core(), backend.core()))
            {
                remote.work_requests = Seen {
                    backend: Some(backend.clone()),
                    connected_at: Some(Instant::now()),
                    ..Default::default()
                };
            }
            let host = Host::Remote(scope.clone());
            if !self
                .tabs
                .values()
                .any(|shown| shown.host == host && !shown.observe)
            {
                // A host with no driving tabs is not polled. Reopening one
                // must baseline its first fresh list, not execute requests
                // accumulated while the window was not observing the host.
                remote.work_requests.pause(Instant::now());
                continue;
            }
            // A retained list from a lost connection cannot establish the
            // new baseline. Wait for a list asked on this connection.
            if !remote.work_requests.fresh(remote.infos.read_at()) {
                continue;
            }
            let listed = remote
                .infos
                .all()
                .flat_map(|info| {
                    info.labels
                        .keys()
                        .filter(|name| request_id(name).is_some())
                        .map(|name| (info.id.clone(), name.clone()))
                })
                .take(MAX_SEEN + 1)
                .collect();
            let new = remote.work_requests.take(listed);
            for info in remote.infos.all() {
                let tokens: Vec<_> = info
                    .labels
                    .iter()
                    .filter(|(name, _)| new.contains(&(info.id.clone(), (*name).clone())))
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect();
                if tokens.is_empty() {
                    continue;
                }
                let shown = self.tabs.iter().find(|(_, shown)| {
                    shown.host == host && !shown.observe && shown.panes.contains(&info.id)
                });
                if let Some((tab, _)) = shown {
                    found.push(LabelRequests {
                        scope: scope.clone(),
                        tab: *tab,
                        terminal: info.id.clone(),
                        tokens,
                    });
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests;
