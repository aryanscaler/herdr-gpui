//! Where a move lands on the destination, and what it leaves on the source:
//! a Herdr 1 workspace made through the `herdr` CLI, or, where the app keeps
//! the workspaces, a checkout made with Git that the app keeps a workspace
//! for once the move ends.

use super::{Candidate, Destination, Keeping, Repository, Review, Source};
use crate::{
    teleport::{
        error::{Error, Result, Step},
        git,
        host::Host,
        snapshot::{SnapshotResult, TabCreated, TabList, WorktreeCreated},
    },
    work,
};
use sheprd_session::Keeper;
use std::sync::atomic::AtomicBool;

/// Where the work lands: a workspace, its checkout, the empty Herdr 1 tab
/// the first source tab is rebuilt in, and Herdr 1 tabs to close once the
/// rebuild is done.
pub(super) struct Landing {
    pub(super) workspace: Landed,
    /// The Herdr 1 workspace its Herdr 1 tabs are made in; none where the
    /// app keeps the workspace, which holds only Herdr 2 tabs.
    pub(super) herdr1: Option<String>,
    pub(super) checkout: String,
    /// A Herdr 1 tab and its pane; a workspace the app keeps has none.
    pub(super) first: Option<(String, String)>,
    pub(super) stale_tabs: Vec<String>,
}

/// The workspace a move landed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landed {
    /// One that exists: a Herdr 1 workspace, or one the app keeps that the
    /// work came back to.
    Workspace(String),
    /// None yet: the app keeps a new one in `checkout`, named `label` when
    /// the source's name was the user's.
    New {
        checkout: String,
        label: Option<String>,
    },
}

/// Put the branch and changes in place: a new worktree, or, coming back, the
/// checkout the work once left (backed up first).
pub(super) fn land(
    source: &Source,
    destination: &Candidate,
    repository: &Repository,
    review: &Review,
    reference: &str,
    report: &mut impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<Landing> {
    let to = &destination.place.host;
    if let Destination::Reclaim {
        workspace_id,
        keeping,
        ..
    } = &destination.destination
    {
        report(Step::Restore);
        return reclaim(to, workspace_id, keeping, reference, report, cancelled);
    }
    git::advance_branch(to, &repository.key, &review.branch, reference, cancelled)?;
    report(Step::CreateWorktree);
    if destination.lands_in == Keeper::App {
        // Git makes it through any checkout of the repository, in the
        // folder Herdr 1 would use; the branch is already in place.
        let main = git::main_checkout(to, &repository.key, cancelled)?;
        let new = work::NewWorktree {
            branch: &review.branch,
            base: "HEAD",
            label: None,
        };
        let created = work::create(
            to,
            &work::Repository::Herdr2 { checkout: main },
            &new,
            cancelled,
        )
        .map_err(|error| match error {
            work::Error::Cancelled => Error::Cancelled,
            error => Error::from(error),
        })?;
        let checkout = created.path;
        report(Step::Restore);
        git::restore(to, &checkout, reference, cancelled)?;
        return Ok(Landing {
            workspace: Landed::New {
                checkout: checkout.clone(),
                label: source.custom_label.clone(),
            },
            herdr1: None,
            checkout,
            first: None,
            stale_tabs: Vec::new(),
        });
    }
    let mut args = vec![
        "worktree",
        "create",
        "--workspace",
        &repository.workspace_id,
        "--branch",
        &review.branch,
        "--no-focus",
    ];
    if let Some(label) = &source.custom_label {
        args.extend(["--label", label]);
    }
    let created: WorktreeCreated = to.herdr(Step::CreateWorktree, &args, cancelled)?;
    report(Step::Restore);
    git::restore(to, &created.worktree.path, reference, cancelled)?;
    Ok(Landing {
        workspace: Landed::Workspace(created.workspace.workspace_id.clone()),
        herdr1: Some(created.workspace.workspace_id),
        checkout: created.worktree.path,
        first: Some((created.tab.tab_id, created.root_pane.pane_id)),
        stale_tabs: Vec::new(),
    })
}

/// Take back the checkout of `workspace_id` the work once left. Its own
/// tabs close once the work's are rebuilt: Herdr 1's here, the app's when
/// the move ends.
fn reclaim(
    to: &Host,
    workspace_id: &str,
    keeping: &Keeping,
    reference: &str,
    report: &mut impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<Landing> {
    let backup = reference.replacen("refs/herdr-teleport/", "refs/herdr-teleport/backup/", 1);
    if let Keeping::App { checkout } = keeping {
        // Its folder may be a subfolder; the whole checkout comes back.
        let checkout = git::toplevel(to, checkout, Step::Restore, cancelled)?;
        git::reclaim(to, &checkout, reference, &backup, cancelled)?;
        return Ok(Landing {
            workspace: Landed::Workspace(workspace_id.to_owned()),
            herdr1: None,
            checkout,
            first: None,
            stale_tabs: Vec::new(),
        });
    }
    let snapshot: SnapshotResult = to.herdr(Step::Restore, &["api", "snapshot"], cancelled)?;
    let snapshot = snapshot.snapshot;
    let checkout = snapshot
        .workspace(workspace_id)
        .and_then(|workspace| workspace.worktree.as_ref())
        .map(|worktree| worktree.checkout_path.clone())
        .ok_or(Error::WorkspaceGone)?;
    let stale_tabs = snapshot
        .tabs_of(workspace_id)
        .iter()
        .map(|tab| tab.tab_id.clone())
        .collect();
    git::reclaim(to, &checkout, reference, &backup, cancelled)?;
    report(Step::Tabs);
    let made: TabCreated = to.herdr(
        Step::Tabs,
        &[
            "tab",
            "create",
            "--workspace",
            workspace_id,
            "--cwd",
            &checkout,
            "--no-focus",
        ],
        cancelled,
    )?;
    Ok(Landing {
        workspace: Landed::Workspace(workspace_id.to_owned()),
        herdr1: Some(workspace_id.to_owned()),
        checkout,
        first: Some((made.tab.tab_id, made.root_pane.pane_id)),
        stale_tabs,
    })
}

/// Stop everything running in the source workspace but keep the workspace.
/// One fresh shell tab replaces a Herdr 1 workspace's tabs; the app does the
/// same for a workspace it keeps, whose Herdr 2 terminals the move ends.
pub(super) fn retire(source: &Source, checkout: &str, cancelled: &AtomicBool) -> Result<()> {
    if let Keeping::App { .. } = source.keeping {
        return Ok(());
    }
    let host = &source.place.host;
    let workspace_id = source.workspace_id.as_str();
    let tabs: TabList = host.herdr(
        Step::Retire,
        &["tab", "list", "--workspace", workspace_id],
        cancelled,
    )?;
    host.herdr_ok(
        Step::Retire,
        &[
            "tab",
            "create",
            "--workspace",
            workspace_id,
            "--cwd",
            checkout,
            "--label",
            "teleported",
            "--no-focus",
        ],
        cancelled,
    )?;
    for tab in tabs.tabs {
        host.herdr_ok(Step::Retire, &["tab", "close", &tab.tab_id], cancelled)?;
    }
    Ok(())
}
