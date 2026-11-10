//! Worktrees of a workspace the app keeps for Herdr 2. herdr-core keeps no
//! worktrees, so the app makes, lists, and removes checkouts itself through
//! `sheprd_remote::work`, as fan-out and dispatch do on Herdr 2, and keeps a
//! workspace for each new checkout, whose first tab lands where the dialog
//! said: its shell, or an agent started in one. Kept workspaces are on this
//! computer, so every script runs here, off the UI thread.

use crate::{
    SheprdWindow,
    i18n::{Localize, fl},
    menu::{worktree_open::Entry, worktree_source::Pending},
};
use gpui::*;
use sheprd_client::ConnectTarget;
use sheprd_remote::{teleport::Host, work};
use std::{path::Path, sync::atomic::AtomicBool};

/// What a kept workspace's new worktree dialog asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    /// A new checkout of `branch`, from `base` unless the branch exists.
    Create {
        branch: String,
        base: String,
        label: Option<String>,
    },
    /// A checkout Git already has, opened as it is.
    Open { path: String },
}

impl Ask {
    /// What a picked row of the dialog asks for, as `pick_request` says for
    /// Herdr 1: an existing checkout opens; a branch, pull request, or
    /// issue gets a new checkout.
    pub(in crate::menu) fn of(pending: &Pending, base: String) -> crate::Result<Self> {
        Ok(match pending {
            Pending::Checkout(path) => Self::Open { path: path.clone() },
            Pending::Branch(branch) => {
                crate::worktree::validate_branch(&branch.name)?;
                Self::Create {
                    branch: branch.name.clone(),
                    base,
                    label: None,
                }
            }
            Pending::Item(item) => {
                let branch = item.branch();
                crate::worktree::validate_branch(&branch)?;
                Self::Create {
                    branch,
                    // A pull request's head may exist only on its remote, so
                    // it starts from the ref the fetch just updated.
                    base: if item.head.is_some() {
                        item.base_ref()
                    } else {
                        base
                    },
                    label: Some(item.label()),
                }
            }
        })
    }
}

/// Git work for a kept workspace that finished off the UI thread.
#[derive(Debug)]
pub(crate) enum KeptDone {
    /// A checkout made or opened for `request`, and where it lands.
    Checkout {
        request: String,
        landing: Box<Landing>,
        result: crate::Result<(Ask, String)>,
    },
    /// A linked checkout removed for `request`, whose kept `workspace`
    /// closes with it.
    Removed {
        request: String,
        /// The host the workspace is kept for, whichever is shown when the
        /// removal finishes.
        scope: crate::browser::Scope,
        workspace: String,
        result: crate::Result<()>,
    },
}

/// Where a new checkout lands, read from the dialog when it was asked for.
#[derive(Debug)]
pub(crate) struct Landing {
    /// The name the dialog gave the workspace, else its folder's.
    label: Option<String>,
    /// The landing chosen; `None` lands in a shell.
    chosen: Option<crate::worktree_landing::Landing>,
    /// The repository group the sidebar unfolds to show the checkout.
    group: Option<String>,
    /// The repository, whose dispatch ranking learns this computer's pick.
    repo: Option<String>,
    /// The device the checkout was asked on, which that ranking names.
    endpoint: String,
    /// The pull request or issue note the new checkout gets.
    note: Option<crate::menu::worktree_source::Note>,
    /// The setup script a new checkout runs once its workspace is kept, as
    /// Herdr 1 runs one in a new worktree's tab; its folder is set once Git
    /// made it.
    setup: Option<Box<crate::worktree_scripts::Launch>>,
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
impl Landing {
    /// A landing in a shell, named `label`, for tests that finish Git work
    /// without running Git.
    pub(crate) fn shell(label: Option<&str>) -> Self {
        Self {
            label: label.map(str::to_owned),
            chosen: None,
            group: None,
            repo: None,
            endpoint: String::new(),
            note: None,
            setup: None,
        }
    }
}

/// The host kept workspaces live on: this computer.
fn this_computer() -> crate::Result<Host> {
    Ok(Host::new(&ConnectTarget::Local)?)
}

/// `checkout`, made or opened as `ask` says. Blocking.
fn carry_out(checkout: &str, ask: &Ask) -> crate::Result<String> {
    match ask {
        Ask::Open { path } => Ok(path.clone()),
        Ask::Create {
            branch,
            base,
            label,
        } => {
            let repository = work::Repository::Herdr2 {
                checkout: checkout.to_owned(),
            };
            let new = work::NewWorktree {
                branch,
                base,
                label: label.as_deref(),
            };
            let host = this_computer()?;
            Ok(work::create(&host, &repository, &new, &AtomicBool::new(false))?.path)
        }
    }
}

impl SheprdWindow {
    /// The folder of the kept workspace the menu is for, which its
    /// repository is read and branched through.
    /// `None` for a workspace kept on another host, whose folder is there.
    pub(in crate::menu) fn kept_checkout(&self) -> Option<String> {
        if self.selected_is_remote() {
            return None;
        }
        let target = self
            .menu
            .target
            .as_ref()
            .filter(|target| target.keeper == sheprd_session::Keeper::App)?;
        match &target.source {
            super::Source::Directory(cwd) if !cwd.is_empty() => Some(cwd.clone()),
            _ => None,
        }
    }

    /// Lists the repository's checkouts for the dialog's Existing tab: those
    /// no kept workspace has open, as Herdr 1 lists the ones none of its
    /// workspaces has.
    pub(in crate::menu) fn list_kept_checkouts(&mut self, cx: &mut Context<Self>) {
        let Some(checkout) = self.kept_checkout() else {
            return;
        };
        // Marked as asked, so a search or tab switch meanwhile does not ask
        // again; no daemon answer can carry this name.
        let Some(source) = self
            .menu
            .worktree
            .as_mut()
            .filter(|s| s.checkouts.request.is_none())
        else {
            return;
        };
        let request = format!("kept-{}", uuid::Uuid::new_v4().simple());
        source.checkouts.request = Some(request.clone());
        let listed = cx.background_executor().spawn(async move {
            let host = this_computer()?;
            Ok::<_, crate::Error>(work::list(&host, &checkout, &AtomicBool::new(false))?)
        });
        cx.spawn(async move |this, cx| {
            let result = listed.await;
            let _ =
                this.update(cx, |this, cx| {
                    let open: Vec<_> = match &result {
                        Ok(checkouts) => checkouts
                            .iter()
                            .map(|checkout| this.kept_workspace_at(Path::new(&checkout.path)))
                            .collect(),
                        Err(_) => Vec::new(),
                    };
                    // Only the dialog that asked: a newer one has its own listing.
                    let Some(source) = this.menu.worktree.as_mut().filter(|source| {
                        source.checkouts.request.as_deref() == Some(request.as_str())
                    }) else {
                        return;
                    };
                    source.checkouts.request = None;
                    match result {
                        Ok(checkouts) => {
                            source.checkouts.entries = checkouts
                                .into_iter()
                                .zip(open)
                                .filter(|(checkout, open)| {
                                    open.is_none() && !checkout.bare && !checkout.prunable
                                })
                                .map(|(checkout, _)| Entry::from(checkout))
                                .collect();
                            source.checkouts.listed = true;
                            source.checkouts.message = None;
                        }
                        Err(error) => source.checkouts.message = Some(error.localize()),
                    }
                    source.refresh();
                    cx.notify();
                });
        })
        .detach();
    }

    /// Makes or opens the checkout `ask` names off the UI thread; the next
    /// tick keeps a workspace for it and lands its first tab as the dialog
    /// chose, even once the dialog has closed. A refusal leaves the dialog
    /// open with Git's own words.
    pub(in crate::menu) fn submit_kept_checkout(&mut self, ask: Ask, cx: &mut Context<Self>) {
        if self.menu.creation.is_some() {
            return;
        }
        let Some(checkout) = self.kept_checkout() else {
            self.menu.error = Some(crate::Error::StaleWorkspace.localize());
            cx.notify();
            return;
        };
        let request = format!("kept-{}", uuid::Uuid::new_v4().simple());
        self.menu.creation = Some(request.clone());
        self.menu.error = None;
        // What lands it is read now: the dialog may close before Git is done.
        let landing = Landing {
            label: match &ask {
                Ask::Create { label, .. } => label.clone().or_else(|| self.worktree_name(cx)),
                Ask::Open { .. } => None,
            },
            // An opened checkout keeps whatever it runs, so it lands in a shell.
            chosen: self
                .menu
                .landing
                .as_ref()
                .filter(|_| matches!(ask, Ask::Create { .. }))
                .map(|picker| picker.chosen),
            group: self
                .menu
                .target
                .as_ref()
                .and_then(|target| target.worktree.as_ref())
                .map(|tree| tree.key.clone()),
            repo: self.menu.dispatch_repo(),
            endpoint: self.endpoints[self.selected_endpoint].id.clone(),
            note: matches!(ask, Ask::Create { .. })
                .then(|| self.worktree_note())
                .flatten(),
            setup: matches!(ask, Ask::Create { .. })
                .then(|| self.kept_setup_launch())
                .flatten()
                .map(Box::new),
        };
        let work = cx
            .background_executor()
            .spawn(async move { carry_out(&checkout, &ask).map(|path| (ask, path)) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.kept_worktree_done.push(KeptDone::Checkout {
                    request,
                    landing: Box::new(landing),
                    result,
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Lands Git work a kept workspace's dialog finished off the UI thread.
    pub(crate) fn poll_kept_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for done in std::mem::take(&mut self.kept_worktree_done) {
            self.land_kept_done(done, window, cx);
        }
    }

    fn land_kept_done(&mut self, done: KeptDone, window: &mut Window, cx: &mut Context<Self>) {
        match done {
            KeptDone::Checkout {
                request,
                landing,
                result,
            } => {
                let current = self.menu.creation.as_deref() == Some(request.as_str());
                if current {
                    self.menu.creation = None;
                }
                match result {
                    Ok((ask, path)) => {
                        self.land_kept_checkout(&ask, *landing, path, current, window, cx)
                    }
                    Err(error) if current => {
                        if let Some(source) = &mut self.menu.worktree {
                            source.pending = None;
                        }
                        self.menu.error = Some(error.localize());
                        cx.notify();
                    }
                    Err(error) => {
                        self.local_error = Some(error.localize());
                        cx.notify();
                    }
                }
            }
            KeptDone::Removed {
                request,
                scope,
                workspace,
                result,
            } => self.finish_kept_removal(&request, &scope, &workspace, result, window, cx),
        }
    }

    /// The made or opened checkout at `path`: its note, its workspace, and
    /// its landing. The open dialog closes when it is the one that asked.
    fn land_kept_checkout(
        &mut self,
        ask: &Ask,
        landing: Landing,
        path: String,
        current: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(ask, Ask::Create { .. }) {
            if let Some(note) = landing.note {
                crate::menu::worktree_source::write_note(note, path.clone(), cx);
            }
            if let Some(mut setup) = landing.setup {
                setup.checkout = Some(crate::worktree_scripts::Checkout {
                    path: path.clone(),
                    root: None,
                });
                self.queue_kept_setup(*setup);
            }
            if let Some(repo) = &landing.repo {
                let endpoint = &landing.endpoint;
                crate::dispatch::History::update(cx, |history| history.record(repo, endpoint));
            }
        }
        if current {
            self.dismiss_menu(window, cx);
        }
        if let Some(group) = &landing.group {
            self.collapsed_repos_mut().remove(group);
        }
        self.land_kept(landing.chosen, landing.label, path.into(), window, cx);
    }

    /// A script of the kept workspace the menu is for, run in a tab of the
    /// kept workspace `workspace` names (empty until a new checkout's opens),
    /// in `checkout` when known. `None` outside a Git checkout.
    pub(in crate::menu) fn kept_script_launch(
        &self,
        kind: crate::worktree_scripts::ScriptKind,
        workspace: String,
        checkout: Option<String>,
    ) -> Option<crate::worktree_scripts::Launch> {
        let target = self.menu.target.as_ref()?;
        let tree = target.worktree.as_ref()?;
        Some(crate::worktree_scripts::Launch {
            kind,
            endpoint: self.menu.endpoint_target,
            endpoint_id: self.endpoints[self.selected_endpoint].id.clone(),
            boot: String::new(),
            workspace,
            repo: tree.label.clone(),
            repo_key: tree.key.clone(),
            checkout: checkout.map(|path| crate::worktree_scripts::Checkout { path, root: None }),
            force: false,
            requested: true,
        })
    }

    /// The setup a new checkout of the menu's kept repository runs: quiet
    /// when the repository has none, as after Herdr 1 makes one.
    fn kept_setup_launch(&self) -> Option<crate::worktree_scripts::Launch> {
        let mut launch = self.kept_script_launch(
            crate::worktree_scripts::ScriptKind::Setup,
            String::new(),
            None,
        )?;
        launch.requested = false;
        Some(launch)
    }

    /// Opens the delete dialog of a kept linked checkout: its folder is the
    /// checkout, so only its archive script is left to read.
    pub(in crate::menu) fn open_kept_deletion(&mut self, cx: &mut Context<Self>) {
        let path = self
            .menu
            .target
            .as_ref()
            .filter(|target| target.can_delete())
            .and(self.kept_checkout());
        let mut deletion = crate::menu::state::Deletion::new(None, false);
        deletion.path = path;
        if deletion.path.is_none() {
            self.menu.error = Some(fl!("menu-state-no-linked-checkout"));
        }
        self.menu.deletion = Some(deletion);
        self.read_archive_script(cx);
    }

    /// Removes the kept linked checkout the delete dialog names off the UI
    /// thread; the next tick closes its workspace, as Herdr 1 closes a
    /// removed worktree's. A checkout with changes stays, and the dialog
    /// offers to discard them.
    pub(in crate::menu) fn remove_kept_worktree(&mut self, cx: &mut Context<Self>) {
        let Some(deletion) = &mut self.menu.deletion else {
            return;
        };
        if deletion.pending.is_some() {
            return;
        }
        let (Some(path), Some(workspace)) = (
            deletion.path.clone(),
            self.menu.target.as_ref().map(|target| target.id.clone()),
        ) else {
            return;
        };
        let force = deletion.force;
        let request = format!("kept-{}", uuid::Uuid::new_v4().simple());
        deletion.pending = Some(request.clone());
        self.menu.error = None;
        self.remove_kept_checkout_for(request, workspace, path, force, None, cx);
    }

    /// Removes the kept linked checkout at `path` off the UI thread, as no
    /// dialog asked, such as once its archive script succeeded or its pull
    /// request merged; the next tick closes its kept `workspace`, or keeps
    /// it and says why.
    pub(crate) fn remove_kept_checkout(
        &mut self,
        workspace: String,
        path: String,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let request = format!("kept-{}", uuid::Uuid::new_v4().simple());
        self.remove_kept_checkout_for(request, workspace, path, force, None, cx);
    }

    pub(crate) fn remove_merged_kept_checkout(
        &mut self,
        workspace: String,
        path: String,
        candidate: crate::merged_cleanup::Candidate,
        cx: &mut Context<Self>,
    ) {
        let request = format!("kept-{}", uuid::Uuid::new_v4().simple());
        self.remove_kept_checkout_for(request, workspace, path, false, Some(candidate), cx);
    }

    fn remove_kept_checkout_for(
        &mut self,
        request: String,
        workspace: String,
        path: String,
        force: bool,
        merged: Option<crate::merged_cleanup::Candidate>,
        cx: &mut Context<Self>,
    ) {
        // Kept for this computer, whichever host is shown when it finishes.
        let index = self.core_host().unwrap_or(self.selected_endpoint);
        let scope = crate::browser::scope(&self.endpoints[index]);
        let removed = cx.background_executor().spawn(async move {
            let host = this_computer()?;
            let cancelled = AtomicBool::new(false);
            if let Some(candidate) = merged {
                crate::merged_cleanup::verify_kept_removal(&candidate, &path, &cancelled)?;
            }
            work::remove(&host, &path, force, &cancelled).map_err(crate::Error::from)
        });
        cx.spawn(async move |this, cx| {
            let result = removed.await;
            let _ = this.update(cx, |this, cx| {
                this.kept_worktree_done.push(KeptDone::Removed {
                    request,
                    scope,
                    workspace,
                    result,
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Closes a removed checkout's workspace, or reports why it stayed in
    /// the dialog that asked, offering force for one with changes.
    fn finish_kept_removal(
        &mut self,
        request: &str,
        scope: &crate::browser::Scope,
        workspace: &str,
        result: crate::Result<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = self
            .menu
            .deletion
            .as_mut()
            .filter(|deletion| deletion.pending.as_deref() == Some(request));
        if let Some(deletion) = asked {
            deletion.pending = None;
            if let Err(error) = &result {
                deletion.force |= matches!(
                    error,
                    crate::Error::Remote(sheprd_remote::Error::Work(work::Error::Dirty))
                );
                self.menu.error = Some(error.localize());
                cx.notify();
                return;
            }
            self.dismiss_menu(window, cx);
        }
        match result {
            Ok(()) => self.close_kept_workspace_in(scope, workspace, window, cx),
            Err(error) => {
                self.local_error = Some(error.localize());
                cx.notify();
            }
        }
    }
}

impl From<work::Checkout> for Entry {
    fn from(checkout: work::Checkout) -> Self {
        let label = Path::new(&checkout.path).file_name().map_or_else(
            || checkout.path.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
        Self {
            label,
            branch: checkout.branch,
            is_bare: checkout.bare,
            is_prunable: checkout.prunable,
            is_detached: checkout.detached,
            open_workspace_id: None,
            path: checkout.path,
        }
    }
}
