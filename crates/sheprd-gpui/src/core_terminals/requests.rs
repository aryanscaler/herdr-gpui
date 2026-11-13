//! Browser requests from agents in herdr-core terminals on another host.
//! The `herdr-gpui` shim there has no Herdr workspace to report to, so it
//! labels its own terminal (`herdr-core label`), under the names a Herdr
//! workspace's tokens have (see `remote_browser`). The window reads them from
//! the list it already keeps of each core, for terminals it shows, and
//! removes them once it acts. Labels stay until removed, so those already
//! there when this window first lists a core predate it and are never
//! served: a restart never replays old requests. Each is read once, so a
//! request left on a terminal no tab here drives is not served when one
//! later shows it, and one being removed is not served again.

use super::{CoreTerminals, Host};
use crate::browser::{Scope, TabId};
use sheprd_backend::TerminalId;
use sheprd_core_client::CoreBackend;
use std::collections::{BTreeMap, HashSet};

/// The labels of one terminal that carry browser requests.
pub(crate) struct LabelRequests {
    pub scope: Scope,
    /// The tab that shows the terminal, whose workspace a page opens in.
    pub tab: TabId,
    pub terminal: TerminalId,
    /// Name and value, as a workspace's tokens are.
    pub tokens: Vec<(String, String)>,
}

impl CoreTerminals {
    pub(super) fn drives_browser_request(&self, scope: &Scope, terminal: &TerminalId) -> bool {
        let host = Host::Remote(scope.clone());
        self.tabs
            .values()
            .any(|tab| tab.host == host && !tab.observe && tab.panes.contains(terminal))
    }

    pub(crate) fn browser_terminal(&self, scope: &Scope, origin: &str) -> Option<TerminalId> {
        let host = Host::Remote(scope.clone());
        self.tabs
            .values()
            .filter(|tab| tab.host == host && !tab.observe)
            .flat_map(|tab| tab.panes.terminals())
            .find(|terminal| crate::chat::core_pane::key_on(&host, terminal) == origin)
            .cloned()
    }

    pub(crate) fn browser_caller_live(&self, scope: &Scope, origin: &str) -> bool {
        self.browser_backend(scope, origin).is_some()
    }

    pub(crate) fn browser_backend(&self, scope: &Scope, origin: &str) -> Option<&CoreBackend> {
        self.browser_terminal(scope, origin)?;
        self.backend_of(&Host::Remote(scope.clone()))
            .filter(|backend| !backend.core().is_lost())
    }

    pub(crate) fn browser_request_value(
        &self,
        scope: &Scope,
        terminal: &TerminalId,
        name: &str,
    ) -> Option<&str> {
        let host = Host::Remote(scope.clone());
        if !self.drives_browser_request(scope, terminal) {
            return None;
        }
        let remote = self.remote.get(scope)?;
        self.backend_of(&host)
            .filter(|backend| !backend.core().is_lost())?;
        remote
            .infos
            .all()
            .find(|info| info.id == *terminal)?
            .labels
            .get(name)
            .map(String::as_str)
    }

    /// Writes a bounded reply on the existing authenticated core connection.
    /// The UI retains the notes until the caller acknowledges reading it.
    pub(crate) fn browser_reply(
        &self,
        scope: &Scope,
        terminal: &TerminalId,
        name: String,
        value: String,
    ) -> Option<std::sync::mpsc::Receiver<sheprd_core_client::Result<()>>> {
        let backend = self.backend_of(&Host::Remote(scope.clone()))?.clone();
        let terminal = terminal.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("browser-reply".into())
            .spawn(move || {
                let result = backend
                    .core()
                    .label(
                        terminal.as_str(),
                        &BTreeMap::from([(name.as_str(), value.as_str())]),
                    )
                    .map(drop);
                let _ = sender.send(result);
            });
        match spawned {
            Ok(_) => Some(receiver),
            Err(error) => {
                tracing::warn!(%error, "Could not start browser reply worker");
                None
            }
        }
    }

    /// Every request label on terminals of other hosts' cores that this
    /// window shows in a tab driving them, each once: a request is served
    /// only the first time it is listed, and never when it predates the
    /// window or was left on a terminal no tab here drove then.
    pub(crate) fn label_requests(&mut self) -> Vec<LabelRequests> {
        let mut found = Vec::new();
        for (scope, remote) in &mut self.remote {
            if remote.infos.lists() == 0 {
                continue;
            }
            let listed: HashSet<(TerminalId, String)> = remote
                .infos
                .all()
                .flat_map(|info| {
                    info.labels
                        .keys()
                        .filter(|name| crate::remote_browser::is_request_token(name))
                        .map(|name| (info.id.clone(), name.clone()))
                })
                .collect();
            let Some(old) = remote.old_requests.as_mut() else {
                remote.old_requests = Some(listed);
                continue;
            };
            // What the core no longer lists is gone for good: request
            // names are never reused.
            old.retain(|request| listed.contains(request));
            let host = Host::Remote(scope.clone());
            for info in remote.infos.all() {
                let tokens: Vec<(String, String)> = info
                    .labels
                    .iter()
                    .filter(|(name, _)| {
                        crate::remote_browser::is_request_token(name)
                            && !old.contains(&(info.id.clone(), (*name).clone()))
                    })
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect();
                if tokens.is_empty() {
                    continue;
                }
                old.extend(
                    tokens
                        .iter()
                        .map(|(name, _)| (info.id.clone(), name.clone())),
                );
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

    /// Removes `names` from `terminal` of the core on `scope`'s host, on a
    /// thread of its own, so the request is not seen again.
    pub(crate) fn clear_labels(&self, scope: &Scope, terminal: &TerminalId, names: Vec<String>) {
        let Some(backend) = self.backend_of(&Host::Remote(scope.clone())).cloned() else {
            return;
        };
        let terminal = terminal.clone();
        let spawned = std::thread::Builder::new()
            .name("herdr-core-label".into())
            .spawn(move || clear(&backend, &terminal, &names));
        if let Err(error) = spawned {
            tracing::warn!(%error, "Could not start a herdr-core label worker");
        }
    }

    /// Whether this window is connected to `host`'s core.
    pub(crate) fn is_connected(&self, host: &Host) -> bool {
        self.backend_of(host).is_some()
    }
}

#[cfg(test)]
impl CoreTerminals {
    /// Takes `list` as what the core on `scope`'s host reports, as a finished
    /// read would.
    pub(crate) fn list_for_test(&mut self, scope: &Scope, list: Vec<sheprd_backend::TerminalInfo>) {
        if let Some(remote) = self.remote.get_mut(scope) {
            remote.infos.list_for_test(list);
        }
    }
}

fn clear(backend: &CoreBackend, terminal: &TerminalId, names: &[String]) {
    let labels: BTreeMap<&str, &str> = names.iter().map(|name| (name.as_str(), "")).collect();
    if let Err(error) = backend.core().label(terminal.as_str(), &labels) {
        tracing::warn!(%error, "Could not clear a browser request from a herdr-core terminal");
    }
}
