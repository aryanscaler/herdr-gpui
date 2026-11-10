//! The window's side of smart dispatch: gathering every host a repository
//! could go to from the endpoints, describing the chosen one to Teleport's
//! scripts, and following a dispatched job to its new workspace.
//!
//! This computer also takes work while it keeps its new workspaces for
//! Herdr 2, with or without Herdr 1 running: Git makes the checkout there and
//! the window keeps the workspace. Another host's herdr-core takes no
//! dispatch yet, so such a host is offline without Herdr 1's snapshot.

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

/// Host scripts need a POSIX client; see `sheprd_client::run_script`.
const SCRIPTABLE_CLIENT: bool = cfg!(any(target_os = "linux", target_os = "macos"));

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

    /// Whether new work dispatched to `endpoint_id` lands in a workspace the
    /// app keeps for Herdr 2: this computer, while its new workspaces are.
    pub(crate) fn dispatch_keeps(&self, endpoint_id: &str) -> bool {
        self.core_host()
            .is_some_and(|index| self.endpoints[index].id == endpoint_id)
            && self.local_workspaces_use_core()
    }

    /// The repositories open in the workspaces the app keeps on endpoint
    /// `index`, one per Git common directory, each reached through its main
    /// checkout's workspace when that is kept, as [`open_repositories`]
    /// reads Herdr 1's.
    fn kept_repositories(&self, index: usize) -> Vec<teleport::job::Repository> {
        let mut found: Vec<teleport::job::Repository> = Vec::new();
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
        if !SCRIPTABLE_CLIENT || host_for(&current.connection.target).is_err() {
            return Vec::new();
        }
        let history = cx.try_global::<History>();
        self.endpoints
            .iter()
            .enumerate()
            .filter(|(index, endpoint)| {
                *index == self.selected_endpoint
                    || (endpoint.enabled && host_for(&endpoint.connection.target).is_ok())
            })
            .map(|(index, endpoint)| {
                let snapshot = self.endpoint_snapshot(index);
                let kept = self
                    .dispatch_keeps(&endpoint.id)
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
                    online: snapshot.is_some() || kept.is_some(),
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
        if !SCRIPTABLE_CLIENT || host_for(&current.connection.target).is_err() {
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
        Some(FreshOrigin {
            place: Place {
                endpoint_id: selected.id.clone(),
                label: selected.label.clone(),
                host: host_for(&selected.connection.target).ok()?,
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
                host: host_for(&endpoint.connection.target).ok()?,
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
            let mut candidates = self.dispatch_candidates(&repo, cx);
            // Fan-out sets other hosts up through a Herdr 1 workspace, so
            // this computer's Herdr 2 takes no lanes from elsewhere yet.
            for candidate in &mut candidates {
                if !candidate.current && self.dispatch_keeps(&candidate.endpoint_id) {
                    candidate.online = false;
                }
            }
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
        let (host, repo) = (job.host.clone(), job.repo.clone());
        self.dispatch_job = None;
        match result {
            Ok(Dispatched::Kept {
                endpoint_id,
                path,
                label,
                checkout,
            }) => {
                History::update(cx, |history| history.record(&repo, &endpoint_id));
                if dialog {
                    self.dismiss_menu(window, cx);
                }
                self.show_flash(
                    Flash::success(fl!("dispatch-created", host = host.clone())),
                    cx,
                );
                self.open_dispatched_kept(label, path.into(), cx);
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
    /// Shows this computer with the kept workspace at `path` a dispatch
    /// made, named `label`, else for its folder.
    fn open_dispatched_kept(
        &mut self,
        label: Option<String>,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let label = label.unwrap_or_else(|| {
            path.file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
        });
        self.open_local_kept_folder(&label, path, cx);
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
