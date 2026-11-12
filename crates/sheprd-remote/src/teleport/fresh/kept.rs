//! Smart dispatch to a host whose terminals herdr-core (Herdr 2) runs.
//! herdr-core keeps no workspaces, so nothing is opened there: the
//! repository is found among the workspaces the app keeps (or an existing
//! checkout, or a clone), the base commit is shipped when it is missing, and
//! Git makes the worktree through `work::create`. The app then keeps the
//! workspace that shows the folder answered.

use super::{Origin, Prepared, ship};
use crate::{
    teleport::{
        error::{Result, Step},
        git,
        job::{self, Destination, HostRepositories},
        provision::{self, Arrival},
    },
    work,
};
use std::sync::atomic::AtomicBool;

/// A checkout of the repository on the destination.
struct Found {
    /// Its folder: a main checkout, or the clone just made.
    path: String,
    /// Its Git common directory.
    key: String,
}

/// The repository on `destination`: a match among `destination`'s
/// repositories, which are the app's kept workspaces (Herdr 1's snapshot is
/// never read), else an existing checkout, else a new clone.
fn find(
    origin: &Origin,
    destination: &HostRepositories,
    report: &mut impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<Found> {
    report(Step::Discover);
    let kept = HostRepositories {
        repositories: Some(destination.repositories.clone().unwrap_or_default()),
        ..destination.clone()
    };
    let source = origin.source();
    let candidate = job::resolve(&source, &kept, cancelled)?;
    let to = &destination.place.host;
    match candidate.destination {
        Destination::Open { repository, .. } | Destination::Reclaim { repository, .. } => {
            Ok(Found {
                path: git::main_checkout(to, &repository.key, cancelled)?,
                key: repository.key,
            })
        }
        Destination::Arrive(Arrival::Existing { path, key }) => Ok(Found { path, key }),
        Destination::Arrive(Arrival::Clone { path }) => {
            report(Step::Clone);
            let key = provision::clone(
                &origin.place.host,
                &origin.repo_key,
                to,
                &path,
                candidate.origin.as_deref(),
                cancelled,
            )?;
            Ok(Found { path, key })
        }
    }
}

/// Prepare a Git checkout without querying or opening a Herdr 1 workspace.
pub(super) fn prepare(
    origin: &Origin,
    destination: &HostRepositories,
    commit: &str,
    report: &mut impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<Prepared> {
    let found = find(origin, destination, report, cancelled)?;
    let host = destination.place.host.clone();
    let reference = ship(origin, &host, &found.key, commit, report, cancelled)?;
    Ok(Prepared {
        repository: job::Repository {
            key: found.key,
            label: origin.repo_label.clone(),
            workspace_id: String::new(),
        },
        work_repository: work::Repository::Herdr2 {
            checkout: found.path,
        },
        host,
        reference,
    })
}

/// A new worktree of `origin`'s repository on `destination`, branched as
/// `branch` from the commit `base` names on the origin. Answers it as its
/// setup script needs it.
pub fn dispatch_kept_worktree(
    origin: &Origin,
    destination: &HostRepositories,
    base: &str,
    branch: &str,
    mut report: impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<super::NewCheckout> {
    let commit = super::base_commit(origin, base, cancelled)?;
    let prepared = prepare(origin, destination, &commit, &mut report, cancelled)?;
    let to = &destination.place.host;
    report(Step::CreateWorktree);
    let new = work::NewWorktree {
        branch,
        base: &commit,
        label: None,
    };
    let created = work::create(to, prepared.work_repository(), &new, cancelled);
    prepared.finish(&AtomicBool::new(false));
    let root = match prepared.work_repository() {
        work::Repository::Herdr2 { checkout } => Some(checkout.clone()),
        work::Repository::Herdr1 { .. } => None,
    };
    Ok(super::NewCheckout {
        repo_key: prepared.repository.key,
        path: created?.path,
        root,
    })
}

/// `origin`'s repository on `destination`, for a new workspace dispatched
/// there: its main checkout, cloned first where it is missing. Answers its
/// folder.
pub fn dispatch_kept_workspace(
    origin: &Origin,
    destination: &HostRepositories,
    mut report: impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<String> {
    Ok(find(origin, destination, &mut report, cancelled)?.path)
}
