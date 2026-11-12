//! Live run status, scoped by host, session and terminal backend.

use crate::{
    SheprdWindow,
    orchestrator::{LiveAgent, LiveBranch, Terminals},
};
use sheprd_client::protocol::ClientShellSnapshot;
use sheprd_session::Session;
use std::sync::Arc;

/// Retaining the inputs prevents allocator address reuse from hiding a new
/// snapshot. Core status and disconnects invalidate independently of Herdr 1.
#[derive(Default)]
pub(crate) struct LiveCache {
    from: Vec<Source>,
    agents: Arc<Vec<LiveAgent>>,
    branches: Arc<Vec<LiveBranch>>,
}

struct Source {
    host: Option<String>,
    session: Option<String>,
    herdr_1: Option<Arc<ClientShellSnapshot>>,
    core: Option<Arc<Session>>,
}

fn same<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.host == other.host
            && self.session == other.session
            && same(&self.herdr_1, &other.herdr_1)
            && same(&self.core, &other.core)
    }
}

impl SheprdWindow {
    pub(super) fn live_agents(&mut self) -> (Arc<Vec<LiveAgent>>, Arc<Vec<LiveBranch>>) {
        let from = self
            .endpoints
            .iter()
            .enumerate()
            .map(|(index, endpoint)| {
                let sessions = self.connected_orchestrator_sessions(index);
                let (host, session) =
                    crate::orchestrator::actions::place(&endpoint.connection.target);
                Source {
                    host,
                    session,
                    herdr_1: sessions
                        .herdr_1
                        .and_then(|_| self.orchestrator_live_state(index).snapshot.clone()),
                    core: sessions.core,
                }
            })
            .collect();
        self.orchestrator_live.update(from);
        (
            self.orchestrator_live.agents.clone(),
            self.orchestrator_live.branches.clone(),
        )
    }
}

impl LiveCache {
    fn update(&mut self, from: Vec<Source>) {
        if from == self.from {
            return;
        }
        let mut agents = Vec::new();
        let mut branches = Vec::new();
        for (endpoint, source) in from.iter().enumerate() {
            if let Some(snapshot) = &source.herdr_1 {
                branches.extend(snapshot.workspaces.iter().filter_map(|workspace| {
                    Some(LiveBranch {
                        host: source.host.clone(),
                        session: source.session.clone(),
                        workspace_id: workspace.workspace_id.clone(),
                        branch: workspace.branch.clone()?,
                    })
                }));
                agents.extend(snapshot.agents.iter().map(|agent| {
                    LiveAgent {
                        terminals: Terminals::Herdr1,
                        endpoint,
                        host: source.host.clone(),
                        session: source.session.clone(),
                        workspace_id: agent.workspace_id.clone(),
                        pane_id: agent.pane_id.clone(),
                        status: agent.agent_status,
                        title: safe_title(
                            agent
                                .title
                                .as_deref()
                                .or(agent.terminal_title_stripped.as_deref()),
                        ),
                    }
                }));
            }
            if let Some(core) = &source.core {
                agents.extend(core.agents.iter().map(|agent| LiveAgent {
                    terminals: Terminals::Herdr2,
                    endpoint,
                    host: source.host.clone(),
                    session: source.session.clone(),
                    workspace_id: String::new(),
                    pane_id: agent.pane.clone(),
                    status: agent.status,
                    title: safe_title(Some(&agent.name)),
                }));
            }
        }
        self.from = from;
        self.agents = Arc::new(agents);
        self.branches = Arc::new(branches);
    }
}

fn safe_title(title: Option<&str>) -> Option<String> {
    title
        .map(|title| crate::notifications::safe_text(title, 160))
        .filter(|title| !title.trim().is_empty())
}

#[cfg(test)]
mod tests;
