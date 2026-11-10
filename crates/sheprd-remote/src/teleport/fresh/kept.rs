//! Smart dispatch to a host whose terminals herdr-core (Herdr 2) runs.
//! herdr-core keeps no workspaces, so nothing is opened there: the
//! repository is found among the workspaces the app keeps (or an existing
//! checkout, or a clone), the base commit is shipped when it is missing, and
//! Git makes the worktree through `work::create`. The app then keeps the
//! workspace that shows the folder answered.

use super::{Origin, ship};
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
    let found = find(origin, destination, &mut report, cancelled)?;
    let to = &destination.place.host;
    let reference = ship(origin, to, &found.key, &commit, &mut report, cancelled)?;
    report(Step::CreateWorktree);
    let repository = work::Repository::Herdr2 {
        checkout: found.path.clone(),
    };
    let new = work::NewWorktree {
        branch,
        base: &commit,
        label: None,
    };
    let created = work::create(to, &repository, &new, cancelled);
    if let Some(reference) = &reference {
        git::drop_reference(to, &found.key, reference, &AtomicBool::new(false));
    }
    Ok(super::NewCheckout {
        repo_key: found.key,
        path: created?.path,
        root: Some(found.path),
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
