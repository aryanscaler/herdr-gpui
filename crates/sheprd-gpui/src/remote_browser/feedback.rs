//! Notes return over the existing core bridge. Request and reply labels belong
//! to the observed terminal on that bridge; no request can nominate a recipient.
//! Notes are retained until an explicit acknowledgement, including on timeout,
//! cancellation, a lost connection, or a failed reply write.

use crate::{
    SheprdWindow,
    browser::{Feedback, FeedbackKey, FeedbackLease, Scope},
};
use gpui::{App, Context, Global, Window};
use sheprd_backend::TerminalId;
use std::time::{Duration, Instant};

const MAX_PENDING: usize = 8;
const ACK_WAIT: Duration = Duration::from_secs(30);

enum Phase {
    Waiting,
    Writing(std::sync::mpsc::Receiver<sheprd_core_client::Result<()>>),
    Offered,
}

struct Pending {
    window: u64,
    target: FeedbackKey,
    terminal: TerminalId,
    request: String,
    reply: String,
    until: Instant,
    phase: Phase,
    notes: Option<FeedbackLease>,
}

#[derive(Default)]
struct PendingReplies(Vec<Pending>);
impl Global for PendingReplies {}

impl Pending {
    /// Advances without I/O. The writer starts bounded background work and
    /// returns its completion channel; no UI thread waits on that channel.
    fn advance(
        &mut self,
        value: Option<&str>,
        now: Instant,
        feedback: &mut Feedback,
        write: impl FnOnce(String) -> Option<std::sync::mpsc::Receiver<sheprd_core_client::Result<()>>>,
    ) -> bool {
        let acknowledged = matches!(self.phase, Phase::Offered) && value == Some("ack");
        let keep = match &self.phase {
            Phase::Waiting if value.is_some_and(|value| value.starts_with("feedback-")) => {
                self.notes = feedback.take_lease(&self.target);
                if self.notes.is_none() && now < self.until {
                    true
                } else if let Some(receiver) = write(
                    serde_json::json!({"notes": self.notes.as_ref().map(FeedbackLease::text)})
                        .to_string(),
                ) {
                    self.phase = Phase::Writing(receiver);
                    self.until = now + ACK_WAIT;
                    true
                } else {
                    false
                }
            }
            Phase::Waiting => false,
            Phase::Writing(receiver) => match receiver.try_recv() {
                Ok(Ok(())) => {
                    self.phase = Phase::Offered;
                    true
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => true,
                Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
            },
            Phase::Offered => value.is_some() && !acknowledged && now < self.until,
        };
        if !keep
            && !acknowledged
            && let Some(notes) = self.notes.take()
        {
            feedback.restore_lease(notes);
        }
        keep
    }
}

/// Forgets every note for `scope` once its saved device names another
/// machine, including notes a waiter holds: a terminal ID on the new machine
/// is not the pane they were written for, so they are never handed to it.
pub(crate) fn retire(scope: &Scope, cx: &mut App) {
    cx.default_global::<PendingReplies>()
        .0
        .retain(|pending| pending.target.scope != *scope);
    cx.default_global::<Feedback>().retire(scope);
}

impl SheprdWindow {
    pub(super) fn begin_remote_feedback(
        &self,
        scope: &Scope,
        terminal: &TerminalId,
        id: &str,
        wait_seconds: u64,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let target = FeedbackKey {
            scope: scope.clone(),
            pane_id: crate::chat::core_pane::key_on(
                &crate::core_terminals::Host::Remote(scope.clone()),
                terminal,
            ),
        };
        let pending = cx.default_global::<PendingReplies>();
        // A second waiter cannot steal the first waiter's offered notes.
        // Refused requests expire in the caller, without consuming notes.
        if pending.0.len() >= MAX_PENDING || pending.0.iter().any(|p| p.target == target) {
            return;
        }
        pending.0.push(Pending {
            window: window.window_handle().window_id().as_u64(),
            target,
            terminal: terminal.clone(),
            request: format!("{}{id}", super::PREFIX),
            reply: format!("sheprd-reply-{id}"),
            until: Instant::now() + Duration::from_secs(wait_seconds.min(600)),
            phase: Phase::Waiting,
            notes: None,
        });
    }

    pub(super) fn poll_remote_feedback(&self, window: &Window, cx: &mut Context<Self>) {
        let window = window.window_handle().window_id().as_u64();
        let pending = std::mem::take(&mut cx.default_global::<PendingReplies>().0);
        let mut remaining = Vec::new();
        for mut pending in pending {
            let owner_open = cx
                .windows()
                .iter()
                .any(|handle| handle.window_id().as_u64() == pending.window);
            if pending.window != window && owner_open {
                remaining.push(pending);
                continue;
            }
            let value = self.core_terminals.browser_request_value(
                &pending.target.scope,
                &pending.terminal,
                &pending.request,
            );
            let scope = pending.target.scope.clone();
            let terminal = pending.terminal.clone();
            let reply = pending.reply.clone();
            let keep = pending.advance(
                value.filter(|_| owner_open),
                Instant::now(),
                cx.default_global::<Feedback>(),
                |text| {
                    self.core_terminals
                        .browser_reply(&scope, &terminal, reply, text)
                },
            );
            if keep {
                remaining.push(pending);
                continue;
            }
            self.core_terminals.clear_labels(
                &pending.target.scope,
                &pending.terminal,
                vec![pending.request, pending.reply],
            );
            cx.notify();
        }
        cx.default_global::<PendingReplies>().0 = remaining;
    }
}

#[cfg(test)]
mod tests;
