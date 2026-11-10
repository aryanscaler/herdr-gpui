//! Opening Teleport from a workspace row: what moves, and the repositories
//! every other connected host has open, both read from the GUI snapshots.
// i18n: migrated

use super::{Page, WorkspaceMenuAction, workspace::popover::Label};
use crate::browser::{Location, Scope, Store, TabId};
use crate::{
    SheprdWindow,
    teleport::{
        Follow, Host, HostRepositories, Keeping, Mark, Place, Retired, Source, Teleport, host_for,
        job::Repository, open_repositories,
    },
};
use gpui::{App, Context, Window};
use sheprd_session::Keeper;
use std::collections::HashMap;

impl SheprdWindow {
    /// Whether the menu's workspace can be teleported: a linked worktree on
    /// a host Teleport can script. It is offered even with no other host
    /// connected, so the dialog can say why there is nowhere to go.
    pub(crate) fn can_teleport(&self) -> bool {
        let Some(target) = &self.menu.target else {
            return false;
        };
        // Host scripts need a POSIX client; see `sheprd_client::run_script`.
        cfg!(any(target_os = "linux", target_os = "macos"))
            && target.can_delete()
            && self
                .teleport
                .as_ref()
                .is_none_or(|teleport| !teleport.moving())
            && host_for(&self.endpoints[self.selected_endpoint].connection.target).is_ok()
    }

    /// The menu's Teleport rows: where the work went once it moved away,
    /// else moving it, and back where it came from.
    pub(super) fn teleport_items(&self) -> Vec<(WorkspaceMenuAction, Label)> {
        if self.teleport_mark().is_some() {
            return vec![
                (WorkspaceMenuAction::GoToTeleported, Label::GO_TO_TELEPORTED),
                (
                    WorkspaceMenuAction::ClearTeleported,
                    Label::CLEAR_TELEPORTED,
                ),
            ];
        }
        let mut items = Vec::new();
        if self.can_teleport() {
            items.push((WorkspaceMenuAction::Teleport, Label::TELEPORT));
            if self.teleport_origin().is_some() {
                items.push((WorkspaceMenuAction::TeleportBack, Label::TELEPORT_BACK));
            }
        }
        items
    }

    /// The teleported mark on the menu's workspace, if its work moved away.
    pub(super) fn teleport_mark(&self) -> Option<&Mark> {
        let target = self.menu.target.as_ref()?;
        self.teleport_marks.find(
            &self.endpoints[self.selected_endpoint].id,
            &target.worktree.as_ref()?.key,
            target.branch.as_deref()?,
        )
    }

    /// Where this copy's work was teleported from, when that host is still
    /// one this window can send it back to.
    pub(super) fn teleport_origin(&self) -> Option<&Mark> {
        let target = self.menu.target.as_ref()?;
        let mark = self.teleport_marks.arrived_at(
            &self.endpoints[self.selected_endpoint].id,
            &target.worktree.as_ref()?.key,
            target.branch.as_deref()?,
            &target.id,
        )?;
        self.endpoints
            .iter()
            .any(|endpoint| endpoint.id == mark.endpoint && endpoint.enabled)
            .then_some(mark)
    }

    /// Teleport, already aimed at the host the work came from.
    pub(super) fn teleport_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(origin) = self.teleport_origin().map(|mark| mark.endpoint.clone()) else {
            return;
        };
        self.open_teleport(window, cx);
        if let Some(teleport) = &mut self.teleport {
            teleport.review_host(&origin);
        }
        cx.notify();
    }

    pub(super) fn go_to_teleported(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mark) = self.teleport_mark().cloned() else {
            return;
        };
        let destination = &mark.destination;
        // A restarted daemon renumbers workspaces; find it by repository and
        // branch when that host's snapshot is at hand.
        let workspace = self
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == destination.endpoint)
            .and_then(|endpoint| endpoint.live.snapshot.as_ref())
            .and_then(|snapshot| {
                snapshot.workspaces.iter().find(|w| {
                    w.branch.as_deref() == Some(mark.branch.as_str())
                        && w.worktree
                            .as_ref()
                            .is_some_and(|t| t.key == destination.repo_key)
                })
            })
            .map_or_else(
                || destination.workspace_id.clone(),
                |w| w.workspace_id.clone(),
            );
        self.dismiss_menu(window, cx);
        self.teleport_follow = Some(Follow::new(destination.endpoint.clone(), workspace));
        cx.notify();
    }

    pub(super) fn clear_teleport_mark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mark) = self.teleport_mark().cloned() {
            self.teleport_marks
                .remove(&mark.endpoint, &mark.repo_key, &mark.branch);
        }
        self.dismiss_menu(window, cx);
    }

    pub(crate) fn open_teleport(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = &self.menu.target else {
            return;
        };
        let Some(worktree) = target.worktree.clone() else {
            return;
        };
        let selected = &self.endpoints[self.selected_endpoint];
        let Ok(host) = host_for(&selected.connection.target) else {
            return;
        };
        // A workspace the app keeps is read from the app; a Herdr 1 one from
        // its snapshot, which names the user's labels.
        let (keeping, custom_label, tab_labels) = if target.keeper == Keeper::App {
            let sessions = self.sessions(self.selected_endpoint);
            let Some(workspace) = sessions
                .core
                .as_ref()
                .and_then(|core| core.workspace(&target.id))
            else {
                return;
            };
            let keeping = Keeping::App {
                checkout: workspace.cwd.clone(),
            };
            let label = workspace.custom_label.then(|| workspace.label.clone());
            (keeping, label, HashMap::new())
        } else {
            let Some(snapshot) = self.live.snapshot.as_ref() else {
                return;
            };
            let label = snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == target.id)
                .filter(|workspace| workspace.custom_label)
                .map(|workspace| workspace.label.clone());
            let tab_labels: HashMap<String, String> = snapshot
                .tabs
                .iter()
                .filter(|tab| tab.workspace_id == target.id && tab.custom_label)
                .map(|tab| (tab.tab_id.clone(), tab.label.clone()))
                .collect();
            (Keeping::Herdr1, label, tab_labels)
        };
        // The workspace's Herdr 2 tabs, whose terminals the host's core runs.
        let moving = core_tabs(cx, &crate::browser::scope(selected), &target.id);
        let source = Source {
            place: Place {
                endpoint_id: selected.id.clone(),
                label: selected.label.clone(),
                host,
            },
            workspace_id: target.id.clone(),
            keeping,
            custom_label,
            repo_key: worktree.key.clone(),
            repo_label: worktree.label.clone(),
            branch: target.branch.clone(),
            tab_labels,
            core_tabs: moving.iter().map(|(_, panes)| panes.clone()).collect(),
            core_preferred: self.config.herdr_2_terminals,
        };
        // Every enabled host, connected or not: Teleport reaches each over its
        // own SSH. A connected host's snapshot saves a round trip; the others
        // are read through their CLI. The same machine listed twice is one host.
        let mut hosts: Vec<HostRepositories> = Vec::new();
        for (index, endpoint) in self.endpoints.iter().enumerate() {
            if index == self.selected_endpoint || !endpoint.enabled {
                continue;
            }
            let Ok(host) = host_for(&endpoint.connection.target) else {
                continue;
            };
            if host == source.place.host || hosts.iter().any(|known| known.place.host == host) {
                continue;
            }
            hosts.push(self.teleport_host(index, host));
        }
        let label = target.label.clone();
        self.menu.page = Some(Page::Teleport);
        self.menu.error = None;
        self.teleport = Some(Teleport::start(source, label, hosts).closing(moving));
        window.focus(&self.menu.focus, cx);
        cx.notify();
    }

    /// The host at `index` as a place to teleport to: the repositories open
    /// there and the checkouts work left, in Herdr 1's workspaces and, on
    /// this computer, those the app keeps for Herdr 2.
    fn teleport_host(&self, index: usize, host: Host) -> HostRepositories {
        let endpoint = &self.endpoints[index];
        let snapshot = endpoint
            .live
            .snapshot
            .as_ref()
            .filter(|_| endpoint.live.status.is_connected());
        let core = self.sessions(index).core;
        let kept = core.as_ref().map_or(&[][..], |core| &core.workspaces[..]);
        // New workspaces there are the app's, as a new workspace on this
        // computer is with Herdr 2 chosen or no Herdr 1 installed.
        let lands_in = if self.core_host() == Some(index) && self.local_workspaces_use_core() {
            Keeper::App
        } else {
            Keeper::Herdr1
        };
        // Checkouts this client teleported away from, still open there.
        let retired: Vec<Retired> = self
            .teleport_marks
            .on(&endpoint.id)
            .filter_map(|mark| {
                let herdr_1 = snapshot.and_then(|snapshot| {
                    snapshot.workspaces.iter().find(|w| {
                        w.branch.as_deref() == Some(mark.branch.as_str())
                            && w.worktree.as_ref().is_some_and(|t| t.key == mark.repo_key)
                    })
                });
                if let Some(workspace) = herdr_1 {
                    return Some(Retired {
                        repo_key: mark.repo_key.clone(),
                        branch: mark.branch.clone(),
                        workspace_id: workspace.workspace_id.clone(),
                        keeping: Keeping::Herdr1,
                    });
                }
                let workspace = kept.iter().find(|w| {
                    w.branch.as_deref() == Some(mark.branch.as_str())
                        && w.worktree.as_ref().is_some_and(|t| t.key == mark.repo_key)
                })?;
                Some(Retired {
                    repo_key: mark.repo_key.clone(),
                    branch: mark.branch.clone(),
                    workspace_id: workspace.id.clone(),
                    keeping: Keeping::App {
                        checkout: workspace.cwd.clone(),
                    },
                })
            })
            .collect();
        // The app's workspaces there open a worktree only where new
        // workspaces are the app's: Herdr 1 knows none of their IDs. A
        // checkout work left comes back through its retired mark either way.
        let mut repositories = snapshot.map(|snapshot| open_repositories(snapshot));
        if lands_in == Keeper::App {
            let found = repositories.get_or_insert_with(Vec::new);
            for workspace in kept {
                let Some(tree) = &workspace.worktree else {
                    continue;
                };
                if !found.iter().any(|repo| repo.key == tree.key) {
                    found.push(Repository {
                        key: tree.key.clone(),
                        label: tree.label.clone(),
                        workspace_id: workspace.id.clone(),
                    });
                }
            }
        }
        HostRepositories {
            place: Place {
                endpoint_id: endpoint.id.clone(),
                label: endpoint.label.clone(),
                host,
            },
            repositories,
            retired,
            lands_in,
        }
    }
}

/// The Herdr 2 tabs of `workspace` that drive their terminals, and their
/// panes. A mirror only watches a terminal listed where it types.
pub(crate) fn core_tabs(
    cx: &App,
    scope: &Scope,
    workspace: &str,
) -> Vec<(TabId, sheprd_session::panes::Panes)> {
    let Some(store) = cx.try_global::<Store>() else {
        return Vec::new();
    };
    store
        .in_workspace(scope, workspace)
        .filter_map(|tab| match &tab.location {
            Some(Location::CoreTerminal {
                panes,
                observe: false,
                ..
            }) => Some((tab.id, panes.clone())),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
