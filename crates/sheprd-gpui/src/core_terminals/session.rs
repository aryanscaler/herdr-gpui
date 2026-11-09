//! The session over herdr-core: the app's herdr-core tabs, gathered in the
//! workspaces the app keeps for them, with the agent each terminal reports.

use crate::{
    SheprdWindow,
    browser::{Location, Scope, Store, scope},
    session_model::Sessions,
};
use gpui::App;
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
        if let Some(store) = cx.try_global::<Store>() {
            for scope in self.remote_core_scopes(store) {
                let session = self.core_session_of(None, store, &scope, cx);
                built.push((scope, Arc::new(session)));
            }
        }
        let unchanged = self.core_terminals.sessions == built;
        if !unchanged {
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
        Session::core(workspaces, tabs, focused, |terminal| {
            self.core_terminals.reported(&host, terminal)
        })
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
