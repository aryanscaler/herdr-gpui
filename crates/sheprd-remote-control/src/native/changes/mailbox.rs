//! Where a phone's question waits for Sheprd, which alone reads the
//! checkouts and keeps Herdr 2 tabs' layouts: the daemon cannot call it, so
//! Sheprd takes what is queued here and answers each by id. Bounded, and a
//! question whose phone gave up is forgotten, whichever side holds it.

use super::Error;
use crate::control::{
    changes::{Answer, Ask, AskId, Failure, Wanted},
    layout,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

/// Questions queued or being answered at once, from every phone.
const MAX_WAITING: usize = 16;
/// How long after Sheprd was last heard from it counts as open: longer than
/// its slowest poll, so a phone does not wait for a Sheprd that is gone.
const OPEN_FOR: Duration = Duration::from_secs(12);

#[derive(Default)]
pub(crate) struct Mailbox {
    arrived: Condvar,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Layout changes queued for Sheprd.
    layout: VecDeque<layout::Ask>,
    next: u64,
    queued: VecDeque<Ask>,
    /// Sheprd was last heard from then.
    seen: Option<Instant>,
    waiting: HashMap<AskId, Waiter>,
}

/// Where an answer goes: the phone waiting for it.
enum Waiter {
    Changes(oneshot::Sender<Answer>),
    Layout(oneshot::Sender<layout::Answer>),
}

/// A question on its way. Dropped, it is withdrawn.
pub(crate) struct Pending<'a, T = Answer> {
    answer: oneshot::Receiver<T>,
    id: AskId,
    mailbox: &'a Mailbox,
}

impl<T> Pending<'_, T> {
    /// Sheprd's answer; `None` if the mailbox dropped the question.
    pub(crate) async fn answer(&mut self) -> Option<T> {
        (&mut self.answer).await.ok()
    }
}

impl<T> Drop for Pending<'_, T> {
    fn drop(&mut self) {
        let mut inner = self.mailbox.inner();
        inner.waiting.remove(&self.id);
        inner.queued.retain(|ask| ask.id != self.id);
        inner.layout.retain(|ask| ask.id != self.id);
    }
}

impl Mailbox {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sheprd is open now.
    pub(crate) fn seen(&self) {
        self.seen_at(Instant::now());
    }

    /// Whether Sheprd is open: it was heard from lately.
    pub(crate) fn open(&self) -> bool {
        open_at(self.inner().seen, Instant::now())
    }

    fn seen_at(&self, now: Instant) {
        self.inner().seen = Some(now);
    }

    /// Queues a question about a workspace of the session `boot_id`.
    pub(crate) fn ask(
        &self,
        boot_id: String,
        workspace_id: String,
        wanted: Wanted,
        now: Instant,
    ) -> Result<Pending<'_>, Error> {
        let mut inner = self.inner();
        let id = inner.admit(now)?;
        let (sender, answer) = oneshot::channel();
        inner.waiting.insert(id, Waiter::Changes(sender));
        inner.queued.push_back(Ask {
            id,
            boot_id,
            workspace_id,
            wanted,
        });
        self.arrived.notify_all();
        Ok(Pending {
            answer,
            id,
            mailbox: self,
        })
    }

    /// Queues a change to the layout of Sheprd's Herdr 2 tab `tab`, at the
    /// pane of `terminal`.
    pub(crate) fn ask_layout(
        &self,
        tab: u64,
        terminal: String,
        op: layout::Op,
        now: Instant,
    ) -> Result<Pending<'_, layout::Answer>, Error> {
        let mut inner = self.inner();
        let id = inner.admit(now)?;
        let (sender, answer) = oneshot::channel();
        inner.waiting.insert(id, Waiter::Layout(sender));
        inner.layout.push_back(layout::Ask {
            id,
            tab,
            terminal,
            op,
        });
        self.arrived.notify_all();
        Ok(Pending {
            answer,
            id,
            mailbox: self,
        })
    }

    /// Takes every queued question, waiting up to `wait` for one. Blocking:
    /// runs on the control connection's own thread.
    pub(crate) fn take(&self, wait: Duration) -> (Vec<Ask>, Vec<layout::Ask>) {
        self.seen();
        let (mut inner, _) = self
            .arrived
            .wait_timeout_while(self.inner(), wait, |inner| {
                inner.queued.is_empty() && inner.layout.is_empty()
            })
            .unwrap_or_else(PoisonError::into_inner);
        inner.seen = Some(Instant::now());
        let asks = inner.queued.drain(..).collect();
        (asks, inner.layout.drain(..).collect())
    }

    /// Hands Sheprd's answer to the phone waiting for it, if one still is.
    /// An answer past the bounds never reaches it.
    pub(crate) fn answer(&self, id: AskId, answer: Answer) {
        let answer = if answer.valid() {
            answer
        } else {
            tracing::warn!("Sheprd's answer about a workspace was out of bounds");
            Answer::Failed(Failure::Unreadable)
        };
        let mut inner = self.inner();
        if let Some(Waiter::Changes(_)) = inner.waiting.get(&id)
            && let Some(Waiter::Changes(waiting)) = inner.waiting.remove(&id)
        {
            let _ = waiting.send(answer);
        }
    }

    /// Hands Sheprd's answer about a layout change to its phone.
    pub(crate) fn answer_layout(&self, id: AskId, answer: layout::Answer) {
        let mut inner = self.inner();
        if let Some(Waiter::Layout(_)) = inner.waiting.get(&id)
            && let Some(Waiter::Layout(waiting)) = inner.waiting.remove(&id)
        {
            let _ = waiting.send(answer);
        }
    }
}

impl Inner {
    /// A new question's id, while Sheprd is open and the queue has room.
    fn admit(&mut self, now: Instant) -> Result<AskId, Error> {
        if !open_at(self.seen, now) {
            return Err(Error::NoDesktop);
        }
        if self.waiting.len() >= MAX_WAITING {
            return Err(Error::Busy);
        }
        let id = AskId(self.next);
        self.next += 1;
        Ok(id)
    }
}

fn open_at(seen: Option<Instant>, now: Instant) -> bool {
    seen.is_some_and(|seen| now.saturating_duration_since(seen) < OPEN_FOR)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
