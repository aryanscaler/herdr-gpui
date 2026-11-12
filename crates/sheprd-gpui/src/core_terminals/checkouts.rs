//! The Git checkout of each workspace the app keeps for Herdr 2: its branch,
//! its repository, and whether it is a linked worktree, which Herdr 1
//! reports for its own workspaces and herdr-core does not. Asked on a worker
//! thread, one batch at a time, and again every so often, as a branch
//! changes under a running shell.

use sheprd_vcs::git::Identity;
use std::{
    collections::HashMap,
    sync::mpsc,
    time::{Duration, Instant},
};

/// How often the folders are asked again.
const REFRESH: Duration = Duration::from_secs(10);
/// How long one folder's answer may take.
const DEADLINE: Duration = Duration::from_secs(5);

type Answers = Vec<(String, Option<Identity>)>;

/// What the folders last answered, and the batch being asked.
#[derive(Default)]
pub(super) struct Checkouts {
    known: HashMap<String, Option<Identity>>,
    asking: Option<mpsc::Receiver<Answers>>,
    asked: Option<Instant>,
}

impl Checkouts {
    /// What `cwd` last answered: `None` while it has not, or when it is no
    /// Git checkout.
    pub(super) fn get(&self, cwd: &str) -> Option<&Identity> {
        self.known.get(cwd)?.as_ref()
    }

    /// Takes a finished batch and starts the next one when `folders` holds
    /// one never asked or the last answers are old. Whether answers changed.
    pub(super) fn poll(&mut self, folders: Vec<String>, now: Instant) -> bool {
        self.poll_on(folders, now, sheprd_client::ConnectTarget::Local)
    }

    pub(super) fn poll_on(
        &mut self,
        folders: Vec<String>,
        now: Instant,
        target: sheprd_client::ConnectTarget,
    ) -> bool {
        let mut changed = false;
        if let Some(asking) = &self.asking {
            match asking.try_recv() {
                Ok(answers) => {
                    for (cwd, identity) in answers {
                        changed |= self.known.get(&cwd) != Some(&identity);
                        self.known.insert(cwd, identity);
                    }
                    self.asking = None;
                }
                Err(mpsc::TryRecvError::Empty) => return false,
                Err(mpsc::TryRecvError::Disconnected) => self.asking = None,
            }
        }
        let before = self.known.len();
        self.known.retain(|cwd, _| folders.contains(cwd));
        changed |= self.known.len() != before;
        let unasked = folders.iter().any(|cwd| !self.known.contains_key(cwd));
        let stale = self.asked.is_none_or(|asked| now >= asked + REFRESH);
        if folders.is_empty() || !(unasked || stale) {
            return changed;
        }
        let (send, receive) = mpsc::sync_channel(1);
        let started = std::thread::Builder::new()
            .name("herdr-core-checkouts".into())
            .spawn(move || {
                let answers = folders
                    .into_iter()
                    .map(|cwd| {
                        let deadline = Instant::now() + DEADLINE;
                        let identity = if target.is_remote() {
                            sheprd_remote::teleport::Host::new(&target)
                                .ok()
                                .and_then(|host| {
                                    sheprd_remote::work::identity_before(
                                        &host,
                                        &cwd,
                                        deadline,
                                        &std::sync::atomic::AtomicBool::new(false),
                                    )
                                    .ok()
                                })
                        } else {
                            sheprd_vcs::git::identity(&cwd, deadline, &|| false)
                                .ok()
                                .flatten()
                        };
                        (cwd, identity)
                    })
                    .collect();
                let _ = send.send(answers);
            });
        match started {
            Ok(_) => self.asking = Some(receive),
            Err(error) => tracing::warn!(%error, "Could not start a checkout worker"),
        }
        self.asked = Some(now);
        changed
    }

    #[cfg(test)]
    pub(crate) fn seed(&mut self, cwd: &str, identity: Option<Identity>) {
        // An older real worker must not overwrite the fixture's answer.
        self.asking = None;
        self.known.insert(cwd.to_owned(), identity);
        self.asked = Some(Instant::now());
    }
}

#[cfg(test)]
impl super::CoreTerminals {
    pub(crate) fn seed_remote_checkout_for_test(
        &mut self,
        scope: &crate::browser::Scope,
        cwd: &str,
        identity: Option<Identity>,
    ) {
        if let Some(remote) = self.remote.get_mut(scope) {
            remote.checkouts.seed(cwd, identity);
        }
    }

    /// Answers for `cwd` as Git would, for tests outside this module.
    pub(crate) fn seed_checkout(&mut self, cwd: &str, identity: Option<Identity>) {
        self.checkouts.seed(cwd, identity);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
