//! A worktree or workspace being created on another host, or on a new
//! machine a provider makes first. It runs on its own named thread and
//! reports through a channel the window drains on its tick. Closing the
//! dialog does not stop it, since stopping halfway could leave a clone or a
//! stray reference behind; only closing the window does.
//!
//! On a destination whose new workspaces the app keeps for Herdr 2 (this
//! computer), herdr-core opens nothing: the job answers the folder made, and
//! the window keeps the workspace that shows it.

use crate::teleport::{self, Created, FreshOrigin, HostRepositories, Naming, Step};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;

enum Event {
    Step(Step),
    /// A step of making the new machine.
    #[cfg(feature = "cloud")]
    Provision(crate::cloud::Step),
    /// The new machine, saved as a device.
    #[cfg(feature = "cloud")]
    Provisioned(crate::cloud::SavedDevice),
    Done(crate::Result<Dispatched>),
}

/// What a dispatch made.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Dispatched {
    /// Herdr 1's new workspace on the destination.
    Herdr1(Created),
    /// A folder on the destination `endpoint_id`, for the window to keep a
    /// Herdr 2 workspace at, named `label` unless it is its folder's.
    Kept {
        endpoint_id: String,
        host_identity: String,
        path: String,
        label: Option<String>,
        /// A new worktree, for its setup script; a workspace has none.
        checkout: Option<teleport::NewCheckout>,
    },
}

/// What a dispatch makes once its destination is ready.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Then {
    Worktree {
        base: String,
        naming: Naming,
    },
    Workspace {
        label: Option<String>,
    },
    /// A worktree on a destination whose workspaces the app keeps: Git needs
    /// the branch named, so an empty name becomes one Herdr 1 would propose.
    KeptWorktree {
        base: String,
        branch: String,
        label: Option<String>,
    },
    /// The repository's main checkout there, as a kept workspace.
    KeptWorkspace {
        label: Option<String>,
    },
}

impl Then {
    /// The same ask, for a destination whose workspaces the app keeps.
    pub(crate) fn kept(self) -> Self {
        match self {
            Self::Worktree { base, naming } => Self::KeptWorktree {
                base,
                branch: naming
                    .branch
                    .unwrap_or_else(crate::worktree::proposed_branch),
                label: naming.label,
            },
            Self::Workspace { label } => Self::KeptWorkspace { label },
            kept => kept,
        }
    }

    fn run(
        &self,
        origin: &FreshOrigin,
        destination: &HostRepositories,
        report: &mut dyn FnMut(Step),
        cancelled: &AtomicBool,
    ) -> crate::Result<Dispatched> {
        let kept = |path, label: &Option<String>, checkout| Dispatched::Kept {
            endpoint_id: destination.place.endpoint_id.clone(),
            host_identity: destination.place.host.identity(),
            path,
            label: label.clone(),
            checkout,
        };
        Ok(match self {
            Self::Worktree { base, naming } => Dispatched::Herdr1(teleport::dispatch_worktree(
                origin,
                destination,
                base,
                naming,
                report,
                cancelled,
            )?),
            Self::Workspace { label } => Dispatched::Herdr1(teleport::dispatch_workspace(
                origin,
                destination,
                label.as_deref(),
                report,
                cancelled,
            )?),
            Self::KeptWorktree {
                base,
                branch,
                label,
            } => {
                let created = teleport::dispatch_kept_worktree(
                    origin,
                    destination,
                    base,
                    branch,
                    report,
                    cancelled,
                )?;
                kept(created.path.clone(), label, Some(created))
            }
            Self::KeptWorkspace { label } => kept(
                teleport::dispatch_kept_workspace(origin, destination, report, cancelled)?,
                label,
                None,
            ),
        })
    }
}

pub(crate) struct Job {
    arrival: Option<Dispatched>,
    arrival_deadline: Option<std::time::Instant>,
    /// The destination's name, for the dialog and the flash.
    pub(crate) host: String,
    /// The repository's name, recorded in the history once the job succeeds.
    pub(crate) repo: String,
    step: Option<Step>,
    #[cfg(feature = "cloud")]
    provision: Option<crate::cloud::Step>,
    /// The new machine, until the window takes it to run its setup script.
    #[cfg(feature = "cloud")]
    provisioned: Option<crate::cloud::SavedDevice>,
    events: mpsc::Receiver<Event>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl Job {
    fn spawn(
        host: String,
        repo: String,
        work: impl FnOnce(&mpsc::Sender<Event>, &AtomicBool) -> crate::Result<Dispatched>
        + Send
        + 'static,
    ) -> Self {
        let (sender, events) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let spawned = std::thread::Builder::new()
            .name("herdr-dispatch".into())
            .spawn(move || {
                let done = work(&sender, &flag);
                let _ = sender.send(Event::Done(done));
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the dispatch worker");
        }
        Self {
            arrival: None,
            arrival_deadline: None,
            host,
            repo,
            step: None,
            #[cfg(feature = "cloud")]
            provision: None,
            #[cfg(feature = "cloud")]
            provisioned: None,
            events,
            cancelled,
        }
    }

    /// Make `then` on `destination`.
    pub(crate) fn on_host(origin: FreshOrigin, destination: HostRepositories, then: Then) -> Self {
        let then = if destination.lands_in == sheprd_session::Keeper::App {
            then.kept()
        } else {
            then
        };
        Self::spawn(
            destination.place.label.clone(),
            origin.repo_label.clone(),
            move |events, cancelled| {
                let mut report = |step| {
                    let _ = events.send(Event::Step(step));
                };
                then.run(&origin, &destination, &mut report, cancelled)
            },
        )
    }

    /// Have `provider` make a new machine, then make `then` there. The
    /// machine is saved as a device as soon as it is ready, so a later
    /// failure leaves it listed, to retry on or remove, rather than unseen.
    #[cfg(feature = "cloud")]
    pub(crate) fn sandboxed(
        provider: crate::cloud::CloudProvider,
        origin: FreshOrigin,
        then: Then,
    ) -> crate::Result<Self> {
        let provision = crate::cloud::provision(provider).ok_or(
            // i18n: data
            crate::cloud::Error::Unavailable("This provider cannot make a machine for a dispatch."),
        )?;
        let host = super::sandbox_words(provider);
        Ok(Self::spawn(
            host,
            origin.repo_label.clone(),
            move |events, cancelled| {
                let stopped = || cancelled.load(Ordering::Acquire);
                let requirement =
                    if matches!(then, Then::KeptWorktree { .. } | Then::KeptWorkspace { .. }) {
                        crate::cloud::Requirement::Core
                    } else {
                        crate::cloud::Requirement::Either
                    };
                // The project's own image, when it has a recipe and the
                // provider holds images, built first if it is not there.
                let image = match crate::cloud::image::checkout_of(&origin.repo_key) {
                    Some(path) => crate::cloud::image::for_new_machine_with(
                        provider,
                        &crate::cloud::image::Checkout {
                            host: &origin.place.host,
                            endpoint_id: &origin.place.endpoint_id,
                            path,
                            label: &origin.repo_label,
                        },
                        requirement,
                        cancelled,
                        &|progress| {
                            let step = crate::cloud::Step::Image(progress.clone());
                            let _ = events.send(Event::Provision(step));
                        },
                    )?,
                    None => None,
                };
                let spec = crate::cloud::Spec {
                    image,
                    requirement,
                    ..crate::cloud::Spec::new()
                };
                let saved = provision(&spec, &stopped, &|step| {
                    let _ = events.send(Event::Provision(step));
                })?;
                let _ = events.send(Event::Provisioned(saved.clone()));
                let destination = HostRepositories {
                    place: teleport::Place {
                        endpoint_id: saved.endpoint_id(),
                        label: saved.label.clone(),
                        host: teleport::host_for(&saved.target())?,
                    },
                    repositories: None,
                    retired: Vec::new(),
                    lands_in: if matches!(
                        then,
                        Then::KeptWorktree { .. } | Then::KeptWorkspace { .. }
                    ) {
                        sheprd_session::Keeper::App
                    } else {
                        sheprd_session::Keeper::Herdr1
                    },
                };
                let mut report = |step| {
                    let _ = events.send(Event::Step(step));
                };
                then.run(&origin, &destination, &mut report, cancelled)
            },
        ))
    }

    /// A job already done with `result`, for tests that land a dispatch
    /// without running its scripts. Only POSIX clients dispatch.
    #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
    pub(crate) fn finished(host: &str, repo: &str, result: crate::Result<Dispatched>) -> Self {
        let (sender, events) = mpsc::channel();
        let _ = sender.send(Event::Done(result));
        Self {
            arrival: None,
            arrival_deadline: None,
            host: host.to_owned(),
            repo: repo.to_owned(),
            step: None,
            #[cfg(feature = "cloud")]
            provision: None,
            #[cfg(feature = "cloud")]
            provisioned: None,
            events,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The new machine, once, for the window to run its setup script.
    #[cfg(feature = "cloud")]
    pub(crate) fn take_provisioned(&mut self) -> Option<crate::cloud::SavedDevice> {
        self.provisioned.take()
    }

    /// Take reported steps. Returns whether the status changed, and the
    /// outcome once, with its failure as display text.
    pub(crate) fn poll(&mut self) -> (bool, Option<Result<Dispatched, String>>) {
        if let Some(arrival) = self.arrival.take() {
            return (false, Some(Ok(arrival)));
        }
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            changed = true;
            match event {
                Event::Step(step) => self.step = Some(step),
                #[cfg(feature = "cloud")]
                Event::Provision(step) => self.provision = Some(step),
                #[cfg(feature = "cloud")]
                Event::Provisioned(saved) => {
                    self.host.clone_from(&saved.label);
                    self.provisioned = Some(saved);
                }
                Event::Done(result) => {
                    return (
                        true,
                        Some(result.map_err(|error| {
                            tracing::warn!(%error, host = %self.host, "dispatch");
                            crate::i18n::Localize::localize(&error)
                        })),
                    );
                }
            }
        }
        (changed, None)
    }

    /// Catalog reads lag provisioning. Retain one completed result while its
    /// saved device appears, without replaying creation or blocking the UI.
    pub(crate) fn await_destination(
        &mut self,
        arrival: Dispatched,
        now: std::time::Instant,
    ) -> Option<Dispatched> {
        let deadline = self
            .arrival_deadline
            .get_or_insert(now + std::time::Duration::from_secs(120));
        if now >= *deadline {
            return Some(arrival);
        }
        self.arrival = Some(arrival);
        None
    }

    /// What the job is doing, for the dialog.
    pub(crate) fn status(&self) -> String {
        #[cfg(feature = "cloud")]
        if let (None, Some(step)) = (self.step, &self.provision) {
            return crate::i18n::fl!(
                "dispatch-making",
                host = self.host.as_str(),
                step = crate::cloud::status::step(step)
            );
        }
        match self.step {
            None => crate::i18n::fl!("dispatch-starting", host = self.host.as_str()),
            Some(step) => crate::i18n::fl!(
                "dispatch-step",
                host = self.host.as_str(),
                step = teleport::text::step(Some(step))
            ),
        }
    }
}
