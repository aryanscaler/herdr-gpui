//! Coming back to a fan-out from its journal record. A restored fan-out is a
//! comparison that does nothing by itself: interrupted lanes are finished,
//! kept, or forgotten only when the user asks, and the prompt is never sent
//! again without that.

use super::{FanOut, LaneState, LaneView, Origin, Stage, job, reporter, spawn, write_later};
use crate::{
    dispatch::Picker,
    fan_out::journal::{Checkpoint, Journal, Record, Resume},
    teleport::Host,
};
use sheprd_session::Keeper;
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Instant,
};

impl FanOut {
    /// Reassociate a missing saved host after it is configured again. This
    /// only restores the comparison; no creation, launch or prompt is replayed.
    pub(crate) fn reconnect(&mut self, host: impl Fn(&str, Keeper) -> Option<Host>) -> bool {
        if !self.restored || !matches!(self.stage, Stage::Compare) {
            return false;
        }
        let mut changed = false;
        for (index, lane) in self.lanes.iter_mut().enumerate() {
            if lane.checkout.is_some() {
                continue;
            }
            let Some(saved) = self.journal.saved_lane(self.id, index) else {
                continue;
            };
            let Some(checkout) = saved.checkout else {
                continue;
            };
            let Some(host) = same(
                &host,
                &checkout.endpoint_id,
                &checkout.host,
                if checkout.herdr_2 {
                    Keeper::App
                } else {
                    Keeper::Herdr1
                },
            ) else {
                continue;
            };
            lane.checkout = Some(checkout.checkout(host));
            lane.state = if saved.step == Checkpoint::Prompted {
                LaneState::Running
            } else {
                LaneState::Interrupted(saved.step)
            };
            changed = true;
        }
        if changed {
            self.next_refresh = None;
        }
        changed
    }

    /// The oldest journaled fan-out no window shows, restored. Records with
    /// nothing left to come back to are forgotten on the way.
    pub(crate) fn reclaim(
        journal: &Journal,
        host: impl Fn(&str, Keeper) -> Option<Host>,
    ) -> Option<Self> {
        loop {
            let origin = |record: &Record| {
                same(
                    &host,
                    &record.origin.endpoint_id,
                    &record.origin.host,
                    if record.origin.checkout.is_some() {
                        Keeper::App
                    } else {
                        Keeper::Herdr1
                    },
                )
            };
            let record = journal.claim(|record| origin(record).is_some())?;
            let id = record.id;
            match Self::restore(record, journal.clone(), &host) {
                Some(fan_out) => return Some(fan_out),
                None => write_later(journal.forget(id)),
            }
        }
    }

    /// The fan-out `record` describes, as a comparison, with each lane where
    /// the journal left it. `host` finds an endpoint's host by ID, used only
    /// when it is the daemon the record saw. `None` when the origin's host
    /// is unknown, a lane cannot be read, or nothing was created that could
    /// need the user.
    pub(crate) fn restore(
        record: Record,
        journal: Journal,
        host: impl Fn(&str, Keeper) -> Option<Host>,
    ) -> Option<Self> {
        if !record.worth_keeping() {
            return None;
        }
        let saved = &record.origin;
        let origin = Origin {
            endpoint_id: saved.endpoint_id.clone(),
            endpoint_label: saved.endpoint_label.clone(),
            host: same(
                &host,
                &saved.endpoint_id,
                &saved.host,
                if saved.checkout.is_some() {
                    Keeper::App
                } else {
                    Keeper::Herdr1
                },
            )?,
            workspace_id: saved.workspace_id.clone(),
            repo_key: saved.repo_key.clone(),
            repo_label: saved.repo_label.clone(),
            base: saved.base.clone(),
            checkout: saved.checkout.clone(),
        };
        // Lanes keep their journal order, which removals index by.
        let lanes = record
            .lanes
            .iter()
            .map(|saved| {
                let lane = saved.lane()?;
                let checkout = saved.checkout.as_ref().and_then(|checkout| {
                    Some(checkout.checkout(same(
                        &host,
                        &checkout.endpoint_id,
                        &checkout.host,
                        if checkout.herdr_2 {
                            Keeper::App
                        } else {
                            Keeper::Herdr1
                        },
                    )?))
                });
                let state = match (&saved.checkout, &checkout, saved.step) {
                    (Some(lost), None, _) => LaneState::Failed(crate::i18n::fl!(
                        "fan-out-lane-lost",
                        host = lost.endpoint_id.as_str(),
                        path = lost.path.as_str()
                    )),
                    (_, _, Checkpoint::Prompted) => LaneState::Running,
                    (_, _, step) => LaneState::Interrupted(step),
                };
                Some(LaneView {
                    lane,
                    state,
                    checkout,
                    stats: None,
                    unread: None,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let (sender, events) = mpsc::channel();
        let hosts = Picker::new(&origin.endpoint_id, Instant::now());
        let lost_prompt = record.prompt.is_none()
            && lanes.iter().any(|lane| match lane.state {
                LaneState::Interrupted(step) => {
                    step.resume().is_some()
                        || (step == Checkpoint::CreatingWorktree
                            && lane
                                .checkout
                                .as_ref()
                                .is_some_and(|checkout| checkout.recoverable))
                }
                _ => false,
            });
        Some(Self {
            origin,
            stage: Stage::Compare,
            picks: Default::default(),
            hosts,
            spread: false,
            overrides: Vec::new(),
            prompt: record.prompt.unwrap_or_default(),
            lanes,
            base: record.commit,
            error: lost_prompt.then(|| crate::i18n::fl!("fan-out-prompt-lost")),
            sender,
            events,
            work: Arc::new(AtomicBool::new(false)),
            probe: Arc::new(AtomicBool::new(false)),
            probing: false,
            next_refresh: None,
            journal,
            id: record.id,
            restored: true,
            wanted: Vec::new(),
            shown: Vec::new(),
            closed: Vec::new(),
        })
    }

    /// How many lanes the app stopped partway through.
    pub(crate) fn interrupted(&self) -> usize {
        self.lanes
            .iter()
            .filter(|lane| matches!(lane.state, LaneState::Interrupted(_)))
            .count()
    }

    /// How lane `index` can be finished, if it can. A lane that failed,
    /// resuming included, is offered again from the last step the journal
    /// saw it reach, so a second try never starts a second agent.
    pub(in crate::fan_out) fn resumable(&self, index: usize) -> Option<Resume> {
        if !matches!(self.stage, Stage::Compare) || self.prompt.is_empty() {
            return None;
        }
        let lane = self.lanes.get(index)?;
        let checkout = lane.checkout.as_ref()?;
        let from = match lane.state {
            LaneState::Interrupted(_) | LaneState::Failed(_) => {
                let step = self.journal.step(self.id, index)?;
                if step == Checkpoint::CreatingWorktree && checkout.recoverable {
                    Resume::Creation
                } else {
                    step.resume()?
                }
            }
            _ => return None,
        };
        // StartingAgent is saved before launching. A missing terminal can
        // mean the app died after the agent started but before saving its
        // address. Never interpret that uncertainty as permission to start
        // another agent: the user can keep the checkout and inspect it.
        if from == Resume::Prompt && checkout.started().is_none() && !checkout.recoverable {
            return None;
        }
        Some(from)
    }

    /// Finish interrupted lane `index`, as the user asked: start its agent
    /// if it never started, then send the prompt.
    pub(in crate::fan_out) fn resume(&mut self, index: usize) -> bool {
        let Some(from) = self.resumable(index) else {
            return false;
        };
        let Some(view) = self.lanes.get_mut(index) else {
            return false;
        };
        let Some(checkout) = view.checkout.clone() else {
            return false;
        };
        view.state = match from {
            Resume::Creation | Resume::Start => LaneState::StartingAgent,
            Resume::Prompt => LaneState::Prompting,
        };
        let lane = view.lane.clone();
        let prompt = self.prompt.clone();
        let report = reporter(self.journal.clone(), self.id, self.sender.clone());
        let cancelled = self.work.clone();
        spawn(move || job::resume(from, index, &lane, &checkout, &prompt, &report, &cancelled));
        self.error = None;
        true
    }

    /// Let the fan-out go without removing anything: its worktrees stay as
    /// ordinary workspaces. Returns whether the window should drop it.
    pub(in crate::fan_out) fn forget(&mut self) -> bool {
        if !matches!(self.stage, Stage::Compare)
            || !self.lanes.iter().all(|lane| lane.state.settled())
        {
            return false;
        }
        self.stop_probe();
        write_later(self.journal.forget(self.id));
        true
    }
}

/// The host of `endpoint_id`, when it is still the daemon `identity` names.
/// Workspace and pane IDs repeat across daemons, so another daemon behind
/// the same endpoint must never be scripted with them.
fn same(
    host: &impl Fn(&str, Keeper) -> Option<Host>,
    endpoint_id: &str,
    identity: &str,
    keeper: Keeper,
) -> Option<Host> {
    host(endpoint_id, keeper).filter(|host| host.identity() == identity)
}
