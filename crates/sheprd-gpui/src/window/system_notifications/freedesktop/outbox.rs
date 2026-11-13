//! One in-flight request, bounded, latest-per-tag pending posts, and the
//! notifications Clear All Notifications closes. Reading replaces_id at
//! dispatch time makes even overlapping first posts replace their
//! predecessor, rather than creating a banner for every event. Closes go
//! first, through the same single task, so a close is never overtaken by a
//! later post.

use super::{Bus, Error, Registry, Session};
use gpui::{App, SystemNotification};
use std::{collections::VecDeque, future::Future};

#[derive(Default)]
pub(in super::super) struct Outbox {
    pending: VecDeque<SystemNotification>,
    /// Server ids to close, oldest first.
    closing: VecDeque<u32>,
    running: bool,
}

/// What the delivery task does next.
enum Job {
    Close(u32),
    Post {
        notification: SystemNotification,
        replaces: u32,
        /// The registry's withdrawals when the post was sent: a reply from
        /// before a withdrawal names a banner that must close.
        withdrawals: u64,
    },
}

impl Outbox {
    /// Whether the caller must start the single delivery task.
    fn enqueue(&mut self, notification: SystemNotification) -> bool {
        self.pending.retain(|old| old.tag != notification.tag);
        if self.pending.len() == super::super::REGISTRY_LIMIT {
            self.pending.pop_front();
        }
        self.pending.push_back(notification);
        !std::mem::replace(&mut self.running, true)
    }

    /// Queues `id` once. Room is left for one id beyond a whole withdrawal:
    /// the reply to the single request in flight, which arrives while the
    /// withdrawal waits, must not push out an id already queued. Only a
    /// further withdrawal before the queue drains (impossible while that
    /// request blocks the task) could drop the oldest.
    fn close(&mut self, id: u32) {
        if self.closing.contains(&id) {
            return;
        }
        if self.closing.len() > super::super::WITHDRAW_LIMIT {
            self.closing.pop_front();
        }
        self.closing.push_back(id);
    }
}

impl Registry {
    fn next_job(&mut self) -> Option<Job> {
        if let Some(id) = self.outbox.closing.pop_front() {
            return Some(Job::Close(id));
        }
        while let Some(notification) = self.outbox.pending.pop_front() {
            // An evicted or withdrawn target must not post after its click
            // routing is gone.
            if let Some(posted) = self.find(&notification.tag) {
                return Some(Job::Post {
                    replaces: posted.bus_id.unwrap_or_default(),
                    notification,
                    withdrawals: self.withdrawals,
                });
            }
        }
        self.outbox.running = false;
        None
    }
}

pub(in super::super) fn post(session: Session, notification: SystemNotification, cx: &mut App) {
    if cx.global_mut::<Registry>().outbox.enqueue(notification) {
        run(session, cx);
    }
}

/// Closes the notifications the server numbered `ids`, as Clear All
/// Notifications does. Without a session nothing was posted over the bus.
pub(in super::super) fn withdraw(ids: impl IntoIterator<Item = u32>, cx: &mut App) {
    let registry = cx.global_mut::<Registry>();
    for id in ids {
        registry.outbox.close(id);
    }
    if registry.outbox.closing.is_empty() || std::mem::replace(&mut registry.outbox.running, true) {
        // Nothing to close, or the running task closes them next.
        return;
    }
    match &registry.bus {
        Bus::Connected(session) => {
            let session = session.clone();
            run(session, cx);
        }
        Bus::Gpui | Bus::Unavailable => {
            registry.outbox.closing.clear();
            registry.outbox.running = false;
        }
    }
}

fn run(session: Session, cx: &mut App) {
    let closer = session.clone();
    run_with(
        cx,
        move |notification, replaces| {
            let session = session.clone();
            async move { super::show(&session, &notification, replaces).await }
        },
        move |id| {
            let session = closer.clone();
            async move { super::close(&session, id).await }
        },
    );
}

/// The single delivery task, with the requests it sends injected.
fn run_with<F, G>(
    cx: &mut App,
    mut send: impl FnMut(SystemNotification, u32) -> F + 'static,
    mut close: impl FnMut(u32) -> G + 'static,
) where
    F: Future<Output = Result<u32, Error>> + 'static,
    G: Future<Output = Result<(), Error>> + 'static,
{
    cx.spawn(async move |cx| {
        while let Some(job) = cx.update(|cx| cx.global_mut::<Registry>().next_job()) {
            match job {
                // A server that already closed it answers with an error.
                Job::Close(id) => {
                    if let Err(error) = close(id).await {
                        tracing::debug!(error = ?error, "Could not close a system notification");
                    }
                }
                Job::Post {
                    notification,
                    replaces,
                    withdrawals,
                } => {
                    let tag = notification.tag.clone();
                    match send(notification, replaces).await {
                        Ok(bus_id) => cx.update(|cx| {
                            let registry = cx.global_mut::<Registry>();
                            if registry.withdrawals == withdrawals {
                                registry.set_bus_id(&tag, bus_id);
                            } else {
                                // Withdrawn while in flight: its id must not
                                // reach a newer post for the same tag, and it
                                // closes before anything posts after it.
                                registry.outbox.close(bus_id);
                            }
                        }),
                        // Keep the previous id on failure, so the next post
                        // can replace it.
                        Err(error) => tracing::warn!(error = ?error, "System notification failed"),
                    }
                }
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests;
