//! The window's side of smart dispatch: gathering every host a repository
//! could go to from the endpoints, describing the chosen one to Teleport's
//! scripts, and following a dispatched job to its new workspace.
//!
//! Hosts keeping their workspaces for Herdr 2 take work without Herdr 1:
//! Git makes the checkout there and the window keeps its workspace.

use super::{Candidate, Dispatched, History, Load, Picker, Repository, Sandbox, Setup};
use crate::{
    SheprdWindow,
    core_terminals::Host,
    i18n::fl,
    menu::Page,
    teleport::{self, FreshOrigin, HostRepositories, Place, host_for, open_repositories},
    window::Flash,
};
use gpui::{App, Context, Window};
use sheprd_client::protocol::{AgentStatus, ClientShellSnapshot};
use std::time::Instant;

/// Windows can run scripts through SSH, WSL and cloud connectors, but has
/// no native local POSIX shell. Check the route rather than hiding every host.
pub(crate) fn scriptable(target: &sheprd_client::ConnectTarget) -> bool {
    scriptable_for(target, sheprd_session::Keeper::Herdr1)
}

pub(crate) fn scriptable_for(
    target: &sheprd_client::ConnectTarget,
    keeper: sheprd_session::Keeper,
) -> bool {
    host_for_keeper(target, keeper).is_some()
}

fn host_for_keeper(
    target: &sheprd_client::ConnectTarget,
    keeper: sheprd_session::Keeper,
) -> Option<teleport::Host> {
    if matches!(target, sheprd_client::ConnectTarget::Wsl { .. }) && !cfg!(windows) {
        return None;
    }
    if !(cfg!(any(target_os = "linux", target_os = "macos"))
        || (cfg!(windows) && target.is_remote()))
    {
        return None;
    }
    match keeper {
        sheprd_session::Keeper::App => sheprd_remote::work::host_for_kept(target).ok(),
        sheprd_session::Keeper::Herdr1 => host_for(target).ok(),
    }
}

fn working(snapshot: &ClientShellSnapshot) -> u32 {
    snapshot
        .agents
        .iter()
        .filter(|agent| agent.agent_status == AgentStatus::Working)
        .count() as u32
}

/// Whether `repo` is open on a host, in Herdr 1's `snapshot` or among the
/// workspaces the app keeps there.
fn repository(
    snapshot: Option<&ClientShellSnapshot>,
    kept: Option<&[teleport::job::Repository]>,
    repo: &str,
) -> Repository {
    if snapshot.is_none() && kept.is_none() {
        return Repository::Unknown;
    }
    let in_snapshot = snapshot.is_some_and(|snapshot| {
        snapshot.workspaces.iter().any(|workspace| {
            workspace
                .worktree
                .as_ref()
                .is_some_and(|tree| tree.label == repo)
        })
    });
    let in_kept = kept.is_some_and(|kept| kept.iter().any(|open| open.label == repo));
    if in_snapshot || in_kept {
        Repository::Present
    } else {
        Repository::Missing
    }
}

impl SheprdWindow {
    fn dispatch_source_keeper(&self) -> sheprd_session::Keeper {
        let kept = self
            .menu
            .target_workspace()
            .is_some_and(|workspace| self.is_kept_workspace(workspace))
            || self.kept_in_front().is_some();
        if kept {
            sheprd_session::Keeper::App
        } else {
            sheprd_session::Keeper::Herdr1
        }
    }

    fn dispatch_keeper(&self, endpoint_id: &str) -> sheprd_session::Keeper {
        if self.dispatch_keeps(endpoint_id) {
            sheprd_session::Keeper::App
        } else {
            sheprd_session::Keeper::Herdr1
        }
    }

    /// The connected snapshot of endpoint `index`, reading the selected one
    /// from the window's own copy.
    pub(crate) fn endpoint_snapshot(&self, index: usize) -> Option<&ClientShellSnapshot> {
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &self.endpoints[index].live
        };
        live.snapshot
            .as_deref()
            .filter(|_| live.status.is_connected())
    }

    /// Snapshot paths and load must belong to the host the scripts address.
    /// A socket can be forwarded, and an unverified local peer proves no
    /// ownership of this computer's files.
    fn dispatch_snapshot(&self, index: usize) -> Option<&ClientShellSnapshot> {
        let endpoint = &self.endpoints[index];
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &endpoint.live
        };
        self.endpoint_snapshot(index).filter(|_| {
            host_for(&endpoint.connection.target).is_ok()
                && (endpoint.connection.target.is_remote() || live.local_daemon_peer)
        })
    }

    /// Whether new work dispatched to `endpoint_id` lands in a workspace the
    /// app keeps for Herdr 2, independent of the window's selected host.
    pub(crate) fn dispatch_keeps(&self, endpoint_id: &str) -> bool {
        let Some(index) = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.id == endpoint_id)
        else {
            return false;
        };
        if Some(index) == self.core_host() {
            return self.local_workspaces_use_core();
        }
        let endpoint = &self.endpoints[index];
        let host = Host::of(&crate::browser::scope(endpoint));
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &endpoint.live
        };
        !self.core_terminals.lacks_core(&host)
            && (self.config.herdr_2_terminals
                || live.herdr_1_unavailable()
                || (self.endpoint_snapshot(index).is_none()
                    && self.core_terminals.is_connected(&host)))
    }

    /// The repositories open in the workspaces the app keeps on endpoint
    /// `index`, one per Git common directory, each reached through its main
    /// checkout's workspace when that is kept, as [`open_repositories`]
    /// reads Herdr 1's.
    fn kept_repositories(&self, index: usize) -> Vec<teleport::job::Repository> {
        // Git checkouts opened in Herdr 1 are valid inputs to Herdr 2 too.
        // The kept preparation path consumes their Git keys, never their IDs.
        let mut found = self
            .dispatch_snapshot(index)
            .map(open_repositories)
            .unwrap_or_default();
        let Some(session) = self.sessions(index).core else {
            return found;
        };
        for workspace in &session.workspaces {
            let Some(tree) = &workspace.worktree else {
                continue;
            };
            match found.iter_mut().find(|open| open.key == tree.key) {
                Some(open) if !tree.linked => open.workspace_id.clone_from(&workspace.id),
                Some(_) => {}
                None => found.push(teleport::job::Repository {
                    key: tree.key.clone(),
                    label: tree.label.clone(),
                    workspace_id: workspace.id.clone(),
                }),
            }
        }
        found
    }

    /// Every host a new checkout of `repo` could go to: the one shown, and
    /// every other enabled host this client can script. Empty when the
    /// shown host cannot be scripted, since its commit could not be shipped.
    pub(crate) fn dispatch_candidates(&self, repo: &str, cx: &App) -> Vec<Candidate> {
        let current = &self.endpoints[self.selected_endpoint];
        if !scriptable_for(&current.connection.target, self.dispatch_source_keeper()) {
            return Vec::new();
        }
        let history = cx.try_global::<History>();
        self.endpoints
            .iter()
            .enumerate()
            .filter(|(index, endpoint)| {
                *index == self.selected_endpoint
                    || (endpoint.enabled
                        && scriptable_for(
                            &endpoint.connection.target,
                            self.dispatch_keeper(&endpoint.id),
                        ))
            })
            .map(|(index, endpoint)| {
                let snapshot = self.dispatch_snapshot(index);
                let host = Host::of(&crate::browser::scope(endpoint));
                let kept = (self.dispatch_keeps(&endpoint.id)
                    && (Some(index) == self.core_host()
                        || self.core_terminals.is_connected(&host)))
                .then(|| self.kept_repositories(index));
                // Of cloud machines, only Azure's load is probed to rank by.
                let load = crate::usage::Host::load_of(&endpoint.connection.target)
                    .and_then(|host| self.system_load.get(&host))
                    .and_then(|reading| reading.latest())
                    .map(|sample| Load {
                        cpu: sample.cpu,
                        cores: sample.cores,
                        load5: sample.load.map(|load| load[1]),
                        memory: sample.memory.map(|memory| memory.percent()),
                    });
                Candidate {
                    endpoint_id: endpoint.id.clone(),
                    label: endpoint.label.clone(),
                    online: if self.dispatch_keeps(&endpoint.id) {
                        kept.is_some()
                    } else {
                        snapshot.is_some()
                    },
                    current: index == self.selected_endpoint,
                    load,
                    // Agents in Herdr's panes and in the host's herdr-core.
                    working: snapshot.map_or(0, working).saturating_add(
                        self.core_terminals
                            .working(&Host::of(&crate::browser::scope(endpoint))),
                    ),
                    repository: repository(snapshot, kept.as_deref(), repo),
                    picks: history.map_or(0, |history| history.picks(repo, &endpoint.id)),
                }
            })
            .collect()
    }

    /// Providers that can make a new machine for a dispatch from the shown
    /// host: only when that host can be scripted, as for other hosts, and
    /// only once the user has a device from the provider, so the dialog
    /// offers nothing they never set up.
    pub(crate) fn dispatch_sandboxes(&self) -> Vec<Sandbox> {
        let current = &self.endpoints[self.selected_endpoint];
        if !scriptable_for(&current.connection.target, self.dispatch_source_keeper()) {
            return Vec::new();
        }
        #[cfg(feature = "cloud")]
        if crate::cloud::unavailable().is_none() {
            let used = |provider| {
                self.endpoints.iter().any(|endpoint| {
                    matches!(&endpoint.connection.target,
                        sheprd_client::ConnectTarget::Cloud { provider: from, .. } if *from == provider)
                })
            };
            return crate::cloud::CloudProvider::ALL
                .iter()
                .copied()
                .filter(|&provider| crate::cloud::provision(provider).is_some() && used(provider))
                .collect();
        }
        Vec::new()
    }

    /// The repository a dispatch branches from, on the shown host.
    pub(crate) fn dispatch_origin(
        &self,
        workspace_id: &str,
        repo_key: &str,
        repo_label: &str,
    ) -> Option<FreshOrigin> {
        let selected = &self.endpoints[self.selected_endpoint];
        let keeper = if self.is_kept_workspace(workspace_id) {
            sheprd_session::Keeper::App
        } else {
            sheprd_session::Keeper::Herdr1
        };
        Some(FreshOrigin {
            place: Place {
                endpoint_id: selected.id.clone(),
                label: selected.label.clone(),
                host: host_for_keeper(&selected.connection.target, keeper)?,
            },
            workspace_id: workspace_id.to_owned(),
            repo_key: repo_key.to_owned(),
            repo_label: repo_label.to_owned(),
        })
    }

    /// The host `endpoint_id` as Teleport's scripts address it, with the
    /// repositories its snapshot shows when it is connected, or on this
    /// computer keeping its workspaces for Herdr 2, the kept ones.
    pub(crate) fn dispatch_destination(&self, endpoint_id: &str) -> Option<HostRepositories> {
        let index = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.id == endpoint_id)?;
        let endpoint = &self.endpoints[index];
        if !scriptable_for(
            &endpoint.connection.target,
            self.dispatch_keeper(endpoint_id),
        ) || (!endpoint.enabled && index != self.selected_endpoint)
        {
            return None;
        }
        self.destination_of(endpoint_id, self.dispatch_keeps(endpoint_id))
    }

    /// The host `endpoint_id` with only the repositories Herdr 1's snapshot
    /// shows there, for callers that create through Herdr 1's CLI on it
    /// (fan-out, the orchestrator): a kept workspace's ID means
    /// nothing to `herdr worktree create`, even on this computer when it
    /// keeps its new workspaces for Herdr 2.
    pub(crate) fn herdr_1_destination(&self, endpoint_id: &str) -> Option<HostRepositories> {
        self.destination_of(endpoint_id, false)
    }

    fn destination_of(&self, endpoint_id: &str, kept: bool) -> Option<HostRepositories> {
        let index = self.endpoints.iter().position(|e| e.id == endpoint_id)?;
        let endpoint = &self.endpoints[index];
        Some(HostRepositories {
            place: Place {
                endpoint_id: endpoint.id.clone(),
                label: endpoint.label.clone(),
                host: host_for_keeper(
                    &endpoint.connection.target,
                    if kept {
                        sheprd_session::Keeper::App
                    } else {
                        sheprd_session::Keeper::Herdr1
                    },
                )?,
            },
            repositories: if kept {
                Some(self.kept_repositories(index))
            } else {
                self.endpoint_snapshot(index).map(open_repositories)
            },
            retired: Vec::new(),
            lands_in: if kept {
                sheprd_session::Keeper::App
            } else {
                sheprd_session::Keeper::Herdr1
            },
        })
    }

    /// Whether a host picker is open, so every host's load must be sampled
    /// even with the CPU display off.
    pub(crate) fn dispatch_sampling(&self) -> bool {
        self.menu.dispatch.is_some()
            || self.orchestrator_sampling
            || self
                .fan_out
                .as_ref()
                .is_some_and(|fan_out| fan_out.composing())
    }

    /// A machine a dispatch made: it runs the setup script a device made in
    /// Settings would, once a window shows it.
    #[cfg(feature = "cloud")]
    fn provisioned_for_dispatch(
        &mut self,
        saved: crate::cloud::SavedDevice,
        cx: &mut Context<Self>,
    ) {
        let Ok(Some(script)) = self.config.cloud.script(saved.provider) else {
            return;
        };
        let pending = crate::cloud_setup::Pending::new(
            saved.endpoint_id(),
            saved.label,
            saved.provider,
            script,
            Instant::now(),
        );
        self.queue_cloud_setup(pending, cx);
    }

    /// Keep open pickers current, and follow a finished job.
    pub(crate) fn poll_dispatch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let dialog = matches!(
            self.menu.page,
            Some(Page::Dialog(
                crate::menu::WorkspaceAction::NewWorktree
                    | crate::menu::WorkspaceAction::NewWorkspace
            ))
        );
        if !dialog {
            self.menu.dispatch = None;
        }
        let repo = self.menu.dispatch_repo();
        if let (Some(repo), true) = (repo, self.menu.dispatch.is_some()) {
            let candidates = self.dispatch_candidates(&repo, cx);
            if let Some(picker) = &mut self.menu.dispatch
                && picker.update(candidates, now)
            {
                cx.notify();
            }
        }
        let composing = self.fan_out.as_ref().filter(|fan_out| fan_out.composing());
        if let Some(repo) = composing.map(|fan_out| fan_out.repo_label().to_owned()) {
            // Fan-out resolves the same typed destination and prepared
            // repository as dispatch, so both generations are eligible.
            let candidates = self.dispatch_candidates(&repo, cx);
            if let Some(fan_out) = &mut self.fan_out
                && fan_out.update_hosts(candidates, now)
            {
                cx.notify();
            }
        }
        self.poll_dispatch_setup(now, cx);
        let Some(job) = &mut self.dispatch_job else {
            return;
        };
        let (changed, finished) = job.poll();
        #[cfg(feature = "cloud")]
        let provisioned = job.take_provisioned();
        #[cfg(feature = "cloud")]
        if let Some(saved) = provisioned {
            self.provisioned_for_dispatch(saved, cx);
        }
        let Some(job) = &mut self.dispatch_job else {
            return;
        };
        let Some(result) = finished else {
            if changed && dialog {
                cx.notify();
            }
            return;
        };
        let result = match result {
            Ok(arrival) => {
                let missing = matches!(&arrival, Dispatched::Kept { endpoint_id, .. }
                    if !self.endpoints.iter().any(|endpoint| endpoint.id == *endpoint_id));
                if !missing {
                    Ok(arrival)
                } else if let Some(arrival) = job.await_destination(arrival, now) {
                    Ok(arrival)
                } else {
                    return;
                }
            }
            result => result,
        };
        let (host, repo) = (job.host.clone(), job.repo.clone());
        self.dispatch_job = None;
        match result {
            Ok(Dispatched::Kept {
                endpoint_id,
                host_identity,
                path,
                label,
                checkout,
            }) => {
                History::update(cx, |history| history.record(&repo, &endpoint_id));
                if dialog {
                    self.dismiss_menu(window, cx);
                }
                if !self.open_dispatched_kept(
                    &endpoint_id,
                    &host_identity,
                    label,
                    path.clone().into(),
                    cx,
                ) {
                    self.show_flash(
                        Flash::warning(fl!(
                            "dispatch-created-unopened",
                            host = host.clone(),
                            path = path
                        )),
                        cx,
                    );
                    return;
                }
                self.show_flash(
                    Flash::success(fl!("dispatch-created", host = host.clone())),
                    cx,
                );
                // Like a local creation, a new worktree runs its setup script
                // once its kept workspace has opened.
                if let Some(checkout) = checkout {
                    self.queue_kept_setup(crate::worktree_scripts::Launch {
                        kind: crate::worktree_scripts::ScriptKind::Setup,
                        endpoint: (
                            self.selection_epoch,
                            self.endpoints[self.selected_endpoint].generation,
                        ),
                        endpoint_id,
                        boot: String::new(),
                        workspace: String::new(),
                        repo,
                        repo_key: checkout.repo_key,
                        checkout: Some(crate::worktree_scripts::Checkout {
                            path: checkout.path,
                            root: checkout.root,
                        }),
                        force: false,
                        requested: false,
                    });
                }
            }
            Ok(Dispatched::Herdr1(created)) => {
                History::update(cx, |history| history.record(&repo, &created.endpoint_id));
                if dialog {
                    self.dismiss_menu(window, cx);
                }
                self.show_flash(
                    Flash::success(fl!("dispatch-created", host = host.clone())),
                    cx,
                );
                // Like a local creation, a new worktree runs its setup script,
                // once the window has followed it there.
                if let Some(checkout) = created.checkout {
                    let setup = Setup::new(
                        created.endpoint_id.clone(),
                        created.workspace_id.clone(),
                        repo,
                        checkout,
                        now,
                    );
                    self.queue_dispatch_setup(setup, cx);
                }
                self.teleport_follow = Some(teleport::Follow::new(
                    created.endpoint_id,
                    created.workspace_id,
                ));
            }
            Err(error) if dialog => {
                self.menu.error = Some(error);
                cx.notify();
            }
            Err(error) => {
                self.show_flash(
                    Flash::warning(fl!(
                        "dispatch-not-created",
                        host = host.clone(),
                        error = error
                    )),
                    cx,
                );
            }
        }
    }
}

impl SheprdWindow {
    /// Shows the destination with the kept workspace at `path` a dispatch
    /// made, named `label`, else for its folder.
    fn open_dispatched_kept(
        &mut self,
        endpoint_id: &str,
        host_identity: &str,
        label: Option<String>,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(endpoint) = self
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == endpoint_id)
        else {
            return false;
        };
        if !host_for_keeper(&endpoint.connection.target, sheprd_session::Keeper::App)
            .is_some_and(|host| host.identity() == host_identity)
            || !self.select_endpoint(endpoint_id, cx)
        {
            return false;
        }
        self.refresh_core_session(cx);
        let label = label.unwrap_or_else(|| {
            path.file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
        });
        match self.kept_workspace_at(&path) {
            Some(id) => {
                self.focus_kept_workspace(&id, cx);
            }
            None => self.new_kept_workspace(Some(&label), Some(path), cx),
        }
        true
    }
}

impl Picker {
    /// A picker for a dialog opened on `current`, filled at once, also
    /// offering a new machine from each of `sandboxes`.
    pub(crate) fn opened(
        current: &str,
        candidates: Vec<Candidate>,
        sandboxes: Vec<Sandbox>,
    ) -> Option<Self> {
        let now = Instant::now();
        let mut picker = Self::new(current, now);
        picker.update(candidates, now);
        picker.offer_sandboxes(sandboxes);
        picker.offers_choice().then_some(picker)
    }
}
