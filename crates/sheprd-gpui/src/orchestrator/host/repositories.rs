//! Repository discovery across the daemon's and app-kept sessions.

use super::scripts_run_on;
use crate::{
    SheprdWindow,
    browser::OrchestratorRepo,
    orchestrator::{RepoSpec, Request, Timing},
    session_model::Sessions,
};

const MAX_REPOS: usize = 32;

impl SheprdWindow {
    /// Each distinct repository, in host and workspace order. Worktrees
    /// share the Git common directory and therefore one database.
    pub(crate) fn repo_specs(&self) -> Vec<RepoSpec> {
        let mut specs: Vec<RepoSpec> = Vec::new();
        // A headless window never reads the user's saved tokens.
        let (azure_store, gitlab_store) = if self.reads_credentials() {
            (self.config.azure_devops.store(), self.config.gitlab.store())
        } else {
            Default::default()
        };
        for (index, endpoint) in self.endpoints.iter().enumerate() {
            let target = &endpoint.connection.target;
            if !endpoint.enabled || !scripts_run_on(target) {
                continue;
            }
            let sessions = self.connected_orchestrator_sessions(index);
            let (token, _) = self.github_account(target);
            for (key, workspace, repo) in repositories(&endpoint.id, &sessions) {
                if specs.iter().any(|spec| spec.key == key) {
                    continue;
                }
                if specs.len() == MAX_REPOS {
                    return specs;
                }
                specs.push(RepoSpec {
                    key,
                    host: endpoint.label.clone(),
                    request: Request {
                        target: target.clone(),
                        checkout: repo.checkout,
                        workspace_id: workspace.id.clone(),
                        token: token.clone(),
                        data_root: None,
                        azure_store,
                        gitlab_store,
                        timing: Timing::default(),
                    },
                });
            }
        }
        specs
    }

    /// A core-only host needs no Herdr 1 connection or snapshot.
    pub(super) fn connected_orchestrator_sessions(&self, index: usize) -> Sessions {
        let mut sessions = self.sessions(index);
        let enabled = self.endpoints[index].enabled;
        if !enabled || !self.orchestrator_live_state(index).status.is_connected() {
            sessions.herdr_1 = None;
        }
        if !enabled || !sessions.core_answers {
            sessions.core = None;
        }
        sessions
    }

    pub(super) fn orchestrator_live_state(&self, index: usize) -> &crate::state::LiveState {
        if index == self.selected_endpoint {
            &self.live
        } else {
            &self.endpoints[index].live
        }
    }

    pub(crate) fn workspace_repo_key(&self, workspace: &str) -> Option<String> {
        let endpoint = self.endpoints.get(self.selected_endpoint)?;
        let sessions = self.sessions(self.selected_endpoint);
        let found = sessions.workspace(workspace)?;
        let repo = OrchestratorRepo::new(found.cwd.clone()).ok()?;
        Some(repo_key(
            &endpoint.id,
            found.worktree.as_ref().map(|tree| tree.key.as_str()),
            &repo.checkout,
        ))
    }

    /// The Git common directory and label smart dispatch uses.
    pub(super) fn orchestrator_repository(&self, id: &str) -> Option<(String, String)> {
        let sessions = self.sessions(self.selected_endpoint);
        let worktree = sessions.workspace(id)?.worktree.as_ref()?;
        Some((worktree.key.clone(), worktree.label.clone()))
    }
}

fn repo_key(endpoint: &str, common: Option<&str>, checkout: &str) -> String {
    format!("{endpoint}\u{1f}{}", common.unwrap_or(checkout))
}

fn repositories<'a>(
    endpoint: &'a str,
    sessions: &'a Sessions,
) -> impl Iterator<Item = (String, &'a sheprd_session::Workspace, OrchestratorRepo)> {
    sessions.workspaces().filter_map(move |workspace| {
        let repo = OrchestratorRepo::new(workspace.cwd.clone()).ok()?;
        let key = repo_key(
            endpoint,
            workspace.worktree.as_ref().map(|tree| tree.key.as_str()),
            &repo.checkout,
        );
        Some((key, workspace, repo))
    })
}

#[cfg(test)]
mod tests;
