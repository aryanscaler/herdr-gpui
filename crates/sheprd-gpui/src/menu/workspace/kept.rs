//! The menu of a workspace the app keeps for Herdr 2. The app carries out
//! what it asks, with no daemon to answer, so it offers what the app can do
//! itself: muting, renaming, and closing the workspace, and new and
//! removed worktrees of its repository, which Git makes
//! (`kept_worktrees.rs`).

use super::{Ask, Label, Page, Source, WorkspaceAction, WorkspaceMenuAction, WorkspaceTarget};
use crate::{SheprdWindow, i18n::Localize};
use gpui::{prelude::*, *};
use sheprd_client::protocol::ClientShellWorktree;
use sheprd_session::Keeper;

/// The rows a kept workspace's menu shows: a note and its checkpoints too
/// once Git says which checkout and branch its folder is, its mute toggle,
/// and new and removed worktrees of its repository, which the app makes
/// with Git.
pub(super) fn items(
    target: &WorkspaceTarget,
    local: bool,
    checkout: bool,
    checkpoints: bool,
    fan_out: Option<Label>,
) -> Vec<(WorkspaceMenuAction, Label)> {
    let mut items = vec![if target.muted {
        (WorkspaceMenuAction::Unmute, Label::UNMUTE)
    } else {
        (WorkspaceMenuAction::Mute, Label::MUTE)
    }];
    // Git runs through host scripts, which need `/bin/sh`, on this
    // computer: another host's kept workspaces wait for its own Git.
    let scripts = cfg!(any(target_os = "linux", target_os = "macos"));
    let git = scripts && local && matches!(target.source, Source::Directory(_));
    if git && target.worktree.is_some() {
        items.push((
            WorkspaceMenuAction::Dialog(WorkspaceAction::NewWorktree),
            Label::NEW_WORKTREE,
        ));
    }
    if let Some(label) = fan_out.filter(|_| git) {
        items.push((WorkspaceMenuAction::FanOut, label));
    }
    items.extend([
        (
            WorkspaceMenuAction::Dialog(WorkspaceAction::Rename),
            Label::RENAME,
        ),
        (
            WorkspaceMenuAction::Dialog(WorkspaceAction::Close),
            Label::CLOSE,
        ),
    ]);
    if checkpoints {
        items.push((WorkspaceMenuAction::Checkpoints, Label::CHECKPOINTS));
    }
    if checkout {
        items.push((
            WorkspaceMenuAction::Dialog(WorkspaceAction::Note),
            Label::NOTE,
        ));
    }
    // As for Herdr 1: a file is read only once asked, so every Git checkout
    // offers its scripts and one without them says so.
    if git && target.worktree.is_some() {
        use crate::worktree_scripts::ScriptKind;
        items.push((
            WorkspaceMenuAction::Script(ScriptKind::Run),
            Label::RUN_SCRIPT,
        ));
        if target.can_delete() {
            items.push((
                WorkspaceMenuAction::Script(ScriptKind::Setup),
                Label::RUN_SETUP_SCRIPT,
            ));
        }
    }
    if git && target.can_delete() {
        items.push((
            WorkspaceMenuAction::Dialog(WorkspaceAction::DeleteWorktree),
            Label::DELETE_WORKTREE,
        ));
    }
    items
}

impl WorkspaceTarget {
    /// A target for a workspace the app keeps.
    fn kept(workspace: &sheprd_session::Workspace) -> Self {
        Self {
            boot_id: String::new(),
            id: workspace.id.clone(),
            label: workspace.label.clone(),
            worktree: workspace.worktree.as_ref().map(|tree| ClientShellWorktree {
                key: tree.key.clone(),
                label: tree.label.clone(),
                is_linked_worktree: tree.linked,
            }),
            close_members: vec![workspace.id.clone()],
            heads_group: false,
            branch: workspace.branch.clone(),
            base: None,
            // Git reads and branches the repository through its folder.
            source: if workspace.cwd.is_empty() {
                Source::Workspace
            } else {
                Source::Directory(workspace.cwd.clone())
            },
            keeper: Keeper::App,
            muted: false,
        }
    }
}

impl SheprdWindow {
    pub(crate) fn open_kept_workspace_menu(
        &mut self,
        id: &str,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sessions = self.sessions(self.selected_endpoint);
        let Some(workspace) = sessions.core.as_ref().and_then(|core| core.workspace(id)) else {
            return;
        };
        self.hover = None;
        self.hover_menu = None;
        self.menu.reset();
        self.menu.endpoint_target = (
            self.selection_epoch,
            self.endpoints[self.selected_endpoint].generation,
        );
        let muted = self.muted_on_selected(crate::notification_mutes::Scope::Workspace, id, cx);
        self.menu.target = Some(WorkspaceTarget {
            muted,
            ..WorkspaceTarget::kept(workspace)
        });
        self.menu.anchor = anchor;
        self.menu.page = Some(Page::Workspace);
        self.discard_composition(cx);
        window.focus(&self.menu.focus, cx);
        cx.notify();
    }

    /// Carries out a kept workspace's dialog at once: the app is its keeper.
    pub(super) fn submit_kept_workspace_dialog(
        &mut self,
        action: WorkspaceAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.menu.target.as_ref().map(|target| target.id.clone()) else {
            return;
        };
        let text = self
            .menu
            .input
            .as_ref()
            .map_or(String::new(), |input| input.text.clone());
        match action {
            WorkspaceAction::Rename => {
                if text.trim().is_empty() {
                    self.menu.error = Some(crate::Error::EmptyWorkspaceLabel.to_string());
                    cx.notify();
                    return;
                }
                self.rename_kept_workspace(&id, &text, cx);
            }
            WorkspaceAction::Close => self.close_kept_workspace(&id, window, cx),
            WorkspaceAction::NewWorktree => {
                let ask = self
                    .menu
                    .target
                    .as_ref()
                    .ok_or(crate::Error::StaleWorkspace)
                    .and_then(|target| Ok((target.base_ref()?, text.trim())))
                    .and_then(|(base, branch)| {
                        crate::worktree::validate_branch(branch)?;
                        Ok(Ask::Create {
                            branch: branch.to_owned(),
                            base,
                            label: None,
                        })
                    });
                match ask {
                    Ok(ask) => self.submit_kept_checkout(ask, cx),
                    Err(error) => {
                        self.menu.error = Some(error.localize());
                        cx.notify();
                    }
                }
                return;
            }
            WorkspaceAction::DeleteWorktree => {
                let Some(deletion) = self
                    .menu
                    .deletion
                    .as_ref()
                    .filter(|deletion| deletion.ready() && deletion.pending.is_none())
                else {
                    return;
                };
                // With an archive script, the checkout is removed once that
                // script's tab succeeds, never before it runs.
                let archive = deletion.archive.script().cloned().and_then(|config| {
                    let kind = crate::worktree_scripts::ScriptKind::Archive;
                    let mut launch =
                        self.kept_script_launch(kind, id.clone(), deletion.path.clone())?;
                    launch.force = deletion.force;
                    // Read from Git with the script, as Herdr 1 names it.
                    if let Some(checkout) = &mut launch.checkout {
                        checkout.root.clone_from(&deletion.root);
                    }
                    Some((launch, config))
                });
                let Some((launch, config)) = archive else {
                    self.remove_kept_worktree(cx);
                    return;
                };
                if let Err(error) = self.start_archive_script(launch, config, cx) {
                    self.menu.error = Some(error.localize());
                    cx.notify();
                    return;
                }
            }
            _ => return,
        }
        self.dismiss_menu(window, cx);
    }
}
