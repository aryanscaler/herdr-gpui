//! The discovery phase: find where on a host the worktree lands, from the
//! checkout it once left to a fresh clone.
use super::{Candidate, Destination, HostRepositories, Keeping, Repository, Source};
use crate::teleport::{
    error::{Error, Result, Step},
    git,
    provision::{self, Arrival},
    remote::{MatchReason, RepoIdentity},
    snapshot::{HostSnapshot, SnapshotResult},
};
use sheprd_session::Keeper;
use std::sync::atomic::AtomicBool;

impl Candidate {
    fn rank(&self) -> (u8, Option<MatchReason>) {
        match &self.destination {
            Destination::Reclaim { .. } => (0, None),
            Destination::Open { reason, .. } => (1, Some(*reason)),
            Destination::Arrive(Arrival::Existing { .. }) => (2, None),
            Destination::Arrive(Arrival::Clone { .. }) => (3, None),
        }
    }
}

/// One repository per Git common directory, reached through its main
/// checkout's workspace when that is open.
pub fn repositories_of(snapshot: &HostSnapshot) -> Vec<Repository> {
    let mut found: Vec<Repository> = Vec::new();
    for workspace in &snapshot.workspaces {
        let Some(tree) = &workspace.worktree else {
            continue;
        };
        match found.iter_mut().find(|repo| repo.key == tree.repo_key) {
            Some(repo) if !tree.is_linked_worktree => {
                repo.workspace_id.clone_from(&workspace.workspace_id);
            }
            Some(_) => {}
            None => found.push(Repository {
                key: tree.repo_key.clone(),
                label: tree.repo_name.clone(),
                workspace_id: workspace.workspace_id.clone(),
            }),
        }
    }
    found
}

/// Why `theirs` is the source's project, if it is. Chosen without asking,
/// a name alone is not enough when there are remotes to compare: that is
/// likely another project.
fn same_project(ours: &RepoIdentity, theirs: &RepoIdentity) -> Option<MatchReason> {
    let reason = ours.matches(theirs)?;
    (reason != MatchReason::Name || ours.remotes.is_empty()).then_some(reason)
}

/// A repository's name from its Git common directory `key`: the folder
/// holding `.git`, or a bare repository's own name without `.git`.
fn repository_name(key: &str) -> String {
    let path = std::path::Path::new(key.trim_end_matches('/'));
    let named = match path.file_name() {
        Some(name) if name == ".git" => path.parent().and_then(std::path::Path::file_name),
        _ => path.file_stem(),
    };
    named.map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// Where the worktree goes on `host`: the checkout it once left, a matching
/// open repository, an existing checkout to open, or a place to clone to.
/// Only the chosen host is looked at, so picking a host is instant.
pub fn resolve(
    source: &Source,
    host: &HostRepositories,
    cancelled: &AtomicBool,
) -> Result<Candidate> {
    let own = git::remotes(
        &source.place.host,
        std::slice::from_ref(&source.repo_key),
        cancelled,
    )?;
    let identity = RepoIdentity {
        name: source.repo_label.clone(),
        remotes: own.get(&source.repo_key).cloned().unwrap_or_default(),
    };
    let origin = provision::source_repository(
        &source.place.host,
        &source.repo_key,
        &source.repo_label,
        cancelled,
    )?;
    let candidates = discover_host(
        host,
        &identity,
        &origin,
        source.branch.as_deref(),
        cancelled,
    )?;
    candidates
        .into_iter()
        .min_by_key(Candidate::rank)
        .ok_or(Error::WorkspaceGone)
}

fn discover_host(
    host: &HostRepositories,
    identity: &RepoIdentity,
    source: &provision::SourceRepository,
    branch: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Vec<Candidate>> {
    let repositories = match &host.repositories {
        Some(repositories) => repositories.clone(),
        None => {
            let snapshot: SnapshotResult =
                host.place
                    .host
                    .herdr(Step::Discover, &["api", "snapshot"], cancelled)?;
            repositories_of(&snapshot.snapshot)
        }
    };
    // The app's retired checkouts are asked too, as no open repository may
    // stand for them.
    let mut keys: Vec<_> = repositories.iter().map(|r| r.key.clone()).collect();
    for retired in &host.retired {
        if matches!(retired.keeping, Keeping::App { .. }) && !keys.contains(&retired.repo_key) {
            keys.push(retired.repo_key.clone());
        }
    }
    let remotes = git::remotes(&host.place.host, &keys, cancelled)?;
    let candidate = |destination| Candidate {
        place: host.place.clone(),
        destination,
        origin: source.origin.clone(),
        lands_in: host.lands_in,
    };
    let open: Vec<Candidate> = repositories
        .into_iter()
        .filter_map(|repository| {
            let theirs = RepoIdentity {
                name: repository.label.clone(),
                remotes: remotes.get(&repository.key).cloned().unwrap_or_default(),
            };
            let reason = same_project(identity, &theirs)?;
            Some(candidate(Destination::Open { repository, reason }))
        })
        .collect();
    // Going back to a checkout the work left: that checkout is the place.
    // One the app keeps is named by its mark alone; its repository is no
    // destination Herdr 1 could open a worktree in.
    let back = branch.and_then(|branch| {
        host.retired.iter().find_map(|retired| {
            if retired.branch != branch {
                return None;
            }
            if let Keeping::App { .. } = retired.keeping {
                // The same branch of another project must never be reset
                // to this work: its repository must be this one.
                let theirs = RepoIdentity {
                    name: repository_name(&retired.repo_key),
                    remotes: remotes.get(&retired.repo_key).cloned().unwrap_or_default(),
                };
                same_project(identity, &theirs)?;
                return Some(Destination::Reclaim {
                    repository: Repository {
                        key: retired.repo_key.clone(),
                        label: identity.name.clone(),
                        workspace_id: retired.workspace_id.clone(),
                    },
                    workspace_id: retired.workspace_id.clone(),
                    keeping: retired.keeping.clone(),
                });
            }
            let repository = open
                .iter()
                .find_map(|candidate| match &candidate.destination {
                    Destination::Open { repository, .. } if repository.key == retired.repo_key => {
                        Some(repository.clone())
                    }
                    _ => None,
                })?;
            (retired.branch == branch).then(|| Destination::Reclaim {
                repository,
                workspace_id: retired.workspace_id.clone(),
                keeping: retired.keeping.clone(),
            })
        })
    });
    if let Some(back) = back {
        // The checkout the work comes back to is kept by whoever kept it.
        let lands_in = match &back {
            Destination::Reclaim {
                keeping: Keeping::App { .. },
                ..
            } => Keeper::App,
            _ => Keeper::Herdr1,
        };
        return Ok(vec![Candidate {
            lands_in,
            ..candidate(back)
        }]);
    }
    if !open.is_empty() {
        return Ok(open);
    }
    let arrival = provision::probe(&host.place.host, source, identity, cancelled)?;
    Ok(vec![candidate(Destination::Arrive(arrival))])
}

#[cfg(test)]
#[path = "tests/discover.rs"]
mod tests;
