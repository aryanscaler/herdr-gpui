//! The session over herdr-core: the app's herdr-core tabs, gathered in the
//! workspaces the app keeps for them, with the agent each terminal reports.

use crate::{
    SheprdWindow,
    browser::{Location, Scope, Store, scope},
    session_model::Sessions,
};
use gpui::App;
use sheprd_backend::TerminalId;
use sheprd_session::{CoreTab, CoreWorkspace, Session, Worktree, panes::Panes};
use std::sync::Arc;

impl SheprdWindow {
    /// The host whose herdr-core session the window keeps: the selected one
    /// when it is this computer, else this computer's first, so its Herdr 2
    /// agents still count while another host is shown.
    pub(crate) fn core_host(&self) -> Option<usize> {
        if !self.selected_is_remote() {
            return Some(self.selected_endpoint);
        }
        self.endpoints
            .iter()
            .position(|endpoint| !endpoint.connection.target.is_remote())
    }

    /// Rebuilds the herdr-core sessions from the store's workspaces and
    /// tabs, the shell shown in place of Herdr 1, and what each core last
    /// reported: this computer's, and each saved host's whose workspaces
    /// the app keeps there. Whether any changed.
    pub(crate) fn refresh_core_session(&mut self, cx: &mut App) -> bool {
        let mut checkouts_changed = false;
        let mut built = Vec::new();
        if let Some(index) = self.core_host() {
            let scope = scope(&self.endpoints[index]);
            self.keep_default_workspace(self.live_at(index).missing_installation, &scope, cx);
            let mut folders: Vec<String> =
                cx.try_global::<Store>().map_or_else(Vec::new, |store| {
                    store
                        .kept_workspaces(&scope)
                        .filter(|workspace| !workspace.cwd.is_empty())
                        .map(|workspace| workspace.cwd.clone())
                        .collect()
                });
            // The shown pane's checkout, for the Git and pull request chips.
            if let Some(folder) = self.shown_core_folder()
                && !folders.contains(&folder)
            {
                folders.push(folder);
            }
            checkouts_changed = self
                .core_terminals
                .checkouts
                .poll(folders, std::time::Instant::now());
            let session = cx
                .try_global::<Store>()
                .map(|store| self.core_session_of(Some(index), store, &scope, cx))
                .unwrap_or_default();
            built.push((scope, Arc::new(session)));
        }
        // A saved host with no Herdr 1 gathers its tabs in a workspace of
        // its own, as this computer does, while its core is not known to be
        // missing too. Kept anew, it opens a shell there, standing in for
        // the one Herdr 1 would show; the workspace goes again, still empty,
        // if that shell does not start.
        for index in 0..self.endpoints.len() {
            let endpoint = &self.endpoints[index];
            if !endpoint.connection.target.is_remote() || !endpoint.enabled {
                continue;
            }
            let scope = scope(endpoint);
            let host = super::Host::of(&scope);
            // A first shell that failed leaves no workspace behind, so it is
            // tried again, once that host's core may be dialled again.
            let missing = self.live_at(index).herdr_1_unavailable()
                && !self.core_terminals.lacks_core(&host)
                && !self
                    .core_terminals
                    .hub_of(&host)
                    .is_some_and(super::Hub::cooling);
            if self.keep_default_workspace(missing, &scope, cx) {
                let purpose = super::Purpose::Tab {
                    scope,
                    workspace: super::WORKSPACE.to_owned(),
                };
                self.start_core_program(purpose, sheprd_backend::Spawn::default(), cx);
            }
        }
        if let Some(store) = cx.try_global::<Store>() {
            for scope in self.remote_core_scopes(store) {
                let session = self.core_session_of(None, store, &scope, cx);
                built.push((scope, Arc::new(session)));
            }
        }
        let unchanged = self.core_terminals.sessions == built;
        if !unchanged {
            let recency = &mut self.core_terminals.recency;
            recency.retain(|scope, _| built.iter().any(|(kept, _)| kept == scope));
            for (scope, session) in &built {
                recency.entry(scope.clone()).or_default().observe(
                    session
                        .agents
                        .iter()
                        .map(|agent| (agent.pane.as_str(), agent.status)),
                );
            }
            self.core_terminals.sessions = built;
        }
        !unchanged || checkouts_changed
    }

    /// The saved hosts whose workspaces the app keeps for their herdr-core,
    /// in the window's order. Their sessions are built whether or not the
    /// host is shown, so the device tree lists them too.
    fn remote_core_scopes(&self, store: &Store) -> Vec<Scope> {
        self.endpoints
            .iter()
            .filter(|endpoint| endpoint.connection.target.is_remote())
            .map(scope)
            .filter(|scope| store.kept_workspaces(scope).next().is_some())
            .collect()
    }

    pub(super) fn live_at(&self, index: usize) -> &crate::state::LiveState {
        if index == self.selected_endpoint {
            &self.live
        } else {
            &self.endpoints[index].live
        }
    }

    /// The session over the core `scope`'s workspaces start in. `local` is
    /// this computer's host, whose folders Git reads here and whose shell
    /// may stand in for Herdr 1; a saved host's has neither.
    fn core_session_of(
        &self,
        local: Option<usize>,
        store: &Store,
        scope: &Scope,
        cx: &App,
    ) -> Session {
        let checkouts = &self.core_terminals.checkouts;
        let host = super::Host::of(scope);
        let workspaces = store.kept_workspaces(scope).map(|saved| {
            let mut workspace = CoreWorkspace::from(saved);
            if local.is_some()
                && let Some(checkout) = checkouts.get(&saved.cwd)
            {
                workspace.branch = checkout.branch.as_deref();
                workspace.worktree = Some(Worktree {
                    key: checkout.key.clone(),
                    label: checkout.label.clone(),
                    linked: checkout.linked,
                });
            }
            workspace
        });
        // The shell shown in place of Herdr 1 is no tab, but runs agents too.
        let shell = local
            .filter(|index| self.live_at(*index).missing_installation)
            .and_then(|_| self.core_fallback_view())
            .map(|view| Panes::single(view.read(cx).terminal().clone()));
        let tabs = store
            .in_scope(scope)
            .filter_map(|tab| match &tab.location {
                Some(Location::CoreTerminal {
                    panes,
                    observe: false,
                    ..
                }) => Some(CoreTab {
                    id: tab.id.number(),
                    panes,
                    title: &tab.title,
                    workspace_id: &tab.workspace_id,
                }),
                _ => None,
            })
            .chain(shell.as_ref().map(|panes| CoreTab {
                id: super::SHELL_TAB,
                panes,
                title: "",
                workspace_id: super::WORKSPACE,
            }));
        let focused = self
            .core_terminals
            .in_front
            .as_ref()
            .filter(|(front, _)| front == scope)
            .map(|(_, id)| id.as_str());
        let mut session = Session::core(workspaces, tabs, focused, |terminal| {
            self.core_terminals.reported(&host, terminal)
        });
        // Which agent each is, and what to call it, by the policy the
        // Overview and the attention service name agents with.
        for agent in &mut session.agents {
            let Some(info) = TerminalId::new(agent.pane.as_str())
                .ok()
                .and_then(|terminal| self.core_terminals.reported(&host, &terminal))
            else {
                continue;
            };
            agent.identity =
                sheprd_core_client::agent_kind(info).map(|kind| kind.name().to_owned());
            if let Some(name) = sheprd_core_client::agent_name(info) {
                agent.name = name;
            }
        }
        session
    }

    /// The pull request and Git status lookup of each kept workspace on the
    /// selected host that is a Git checkout on a branch, by workspace.
    pub(crate) fn kept_checkout_inputs(&self) -> Vec<(String, crate::pull_request::Input)> {
        let Some(core) = self.sessions(self.selected_endpoint).core else {
            return Vec::new();
        };
        core.workspaces
            .iter()
            .filter_map(|workspace| {
                let tree = workspace.worktree.as_ref()?;
                let input =
                    crate::pull_request::key_input(&tree.key, workspace.branch.as_deref()).ok()?;
                Some((workspace.id.clone(), input))
            })
            .collect()
    }

    /// Puts `session` in place of the herdr-core session of this computer's
    /// host, as a core reporting its agents would.
    #[cfg(test)]
    pub(crate) fn set_core_session_for_test(&mut self, session: Session) {
        let Some(index) = self.core_host() else {
            return;
        };
        let scope = scope(&self.endpoints[index]);
        let sessions = &mut self.core_terminals.sessions;
        sessions.retain(|(known, _)| *known != scope);
        sessions.insert(0, (scope, Arc::new(session)));
    }

    /// The Herdr 2 terminal the user is in, for the agent steps to start
    /// from: the pane holding the keyboard, the stand-in shell when it does,
    /// else, unless a Herdr 1 pane has it, the pane in use of the Herdr 2
    /// tab shown, such as one whose native chat holds the keyboard.
    pub(crate) fn current_core_terminal(
        &self,
        window: &gpui::Window,
        cx: &App,
    ) -> Option<TerminalId> {
        if let Some((_, terminal)) = self.core_terminals.focused_pane(window, cx) {
            return Some(terminal);
        }
        if let Some(view) = self.core_fallback_view()
            && view.read(cx).focus_handle().contains_focused(window, cx)
        {
            return Some(view.read(cx).terminal().clone());
        }
        if self.focus.is_focused(window) {
            return None;
        }
        self.core_terminals
            .shown
            .as_ref()
            .map(|(_, terminal)| terminal.clone())
    }

    /// Drops what the window restored of every tab, as at launch before a
    /// workspace is shown.
    #[cfg(test)]
    pub(crate) fn forget_core_tabs_for_test(&mut self) {
        self.core_terminals.tabs.clear();
        self.core_terminals.views.clear();
    }

    /// The Herdr 2 agents of the host at `index` as the sidebar lists them,
    /// while the app keeps a session over that host's core.
    pub(crate) fn listed_core_agents(
        &self,
        index: usize,
    ) -> Option<crate::sidebar::CoreAgents<'_>> {
        let scope = scope(self.endpoints.get(index)?);
        let session = self.core_terminals.session_of(&scope)?;
        let host = super::Host::of(&scope);
        // The pane in use only marks this host's agent: IDs repeat across
        // cores.
        let shown = self
            .core_terminals
            .shown
            .as_ref()
            .filter(|(tab, _)| self.core_terminals.host_of(*tab) == host)
            .map(|(_, terminal)| terminal.as_str());
        let snapshot = self.live_at(index).snapshot.as_deref();
        let agents = session
            .agents
            .iter()
            .map(|agent| crate::sidebar::CoreAgent {
                agent,
                session,
                snapshot,
                info: TerminalId::new(agent.pane.as_str())
                    .ok()
                    .and_then(|terminal| self.core_terminals.reported(&host, &terminal)),
                focused: shown == Some(agent.pane.as_str()),
            })
            .collect();
        Some(crate::sidebar::CoreAgents {
            index,
            answers: self.core_terminals.backend_of(&host).is_some(),
            agents,
            recency: self.core_terminals.recency.get(&scope),
        })
    }

    /// The sessions of the host at `index`: Herdr 1's, and herdr-core's
    /// when the app keeps a session over that host's core.
    pub(crate) fn sessions(&self, index: usize) -> Sessions {
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &self.endpoints[index].live
        };
        let scope = scope(&self.endpoints[index]);
        let core = self.core_terminals.session_of(&scope).cloned();
        let core_answers = self
            .core_terminals
            .backend_of(&super::Host::of(&scope))
            .is_some();
        Sessions {
            core,
            core_answers,
            herdr_1: live.session(),
        }
    }
}
