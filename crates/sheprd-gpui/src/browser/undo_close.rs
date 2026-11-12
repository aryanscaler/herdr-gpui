//! Undo for closing a Herdr terminal tab, with `[undo_close_tab]` on.
//!
//! Herdr ends a tab's programs as it closes it, so a tab can only come back
//! as it was if it never closed. A close therefore waits: the window moves
//! on to a neighbour and hides the tab, as tearing one off does (see
//! [`super::hidden`]), and Herdr is asked to close it only once the undo
//! window passes. Reopen Closed Tab within it lists the tab again, its
//! programs and scrollback as they were. After it, the tab is recorded like
//! any other close, so Reopen Closed Tab opens a new shell where it stood.
//!
//! Only this window hides the tab: Herdr's other clients list it until it
//! closes. One focused again in this window, as by Next Tab, is listed
//! again rather than closed under the user. A workspace's last tab closes at
//! once, since Herdr closes the workspace with it, and so does a tab alone
//! in its group, which has no neighbour to show in its place. As the app
//! quits or the window closes, waiting closes are queued at once, but only
//! queued: the connection drops queued work as it stops, so a close behind
//! another request, or on a connection already gone, leaves its tab open.
use super::{Pick, Scope, Store, view::scope};
use crate::{SheprdWindow, browser::ClosedHerdrTab};
use gpui::{Context, Task, Window};
use serde_json::json;
use sheprd_client::Method;
use std::time::{Duration, Instant};

/// How soon a close that could not be queued is tried again, when no tick
/// comes first.
const RETRY: Duration = Duration::from_millis(100);

/// A tab whose close waits for the undo window to pass.
#[derive(Debug)]
struct Pending {
    scope: Scope,
    boot: String,
    closed: ClosedHerdrTab,
    due: Instant,
}

impl Pending {
    fn pick(&self) -> Pick {
        Pick::Herdr(self.closed.tab_id.clone())
    }

    fn key(&self) -> (Scope, String) {
        (self.scope.clone(), self.closed.workspace_id.clone())
    }
}

/// The window's waiting closes, oldest first, and the wake for the next,
/// with the time it is for.
#[derive(Default)]
pub(crate) struct UndoClose {
    pending: Vec<Pending>,
    wake: Option<(Instant, Task<()>)>,
}

impl UndoClose {
    /// Whether the close of Herdr tab `tab` on the host `scope` is waiting,
    /// so it counts as closed. Hosts number their tabs alike.
    pub(crate) fn waits(&self, scope: &Scope, tab: &str) -> bool {
        self.pending
            .iter()
            .any(|pending| pending.scope == *scope && pending.closed.tab_id == tab)
    }
}

/// What became of a waiting close on a poll.
enum Outcome {
    Wait,
    /// Herdr was asked to close it, or it is already gone.
    Done,
    /// Listed again: focused anew, its close could not be sent, or the
    /// daemon restarted under it.
    Restore,
}

impl SheprdWindow {
    /// Hides `closed` and closes it once the undo window passes, when
    /// `[undo_close_tab]` is on and the tab is not its workspace's last.
    /// Returns whether it did, so the caller sends no close of its own.
    pub(crate) fn defer_tab_close(
        &mut self,
        closed: &ClosedHerdrTab,
        cx: &mut Context<Self>,
    ) -> bool {
        let undo = self.config.undo_close_tab;
        let Some(snapshot) = self.live.snapshot.as_ref().filter(|_| undo.enabled) else {
            return false;
        };
        let host = scope(&self.endpoints[self.selected_endpoint]);
        let undo_close = &self.browser.undo;
        let others = snapshot.tabs.iter().any(|tab| {
            tab.workspace_id == closed.workspace_id
                && tab.tab_id != closed.tab_id
                && !undo_close.waits(&host, &tab.tab_id)
        });
        if !others {
            return false;
        }
        let pending = Pending {
            scope: host,
            boot: snapshot.boot_id.clone(),
            closed: closed.clone(),
            due: Instant::now() + undo.window,
        };
        let pick = pending.pick();
        self.leave_tab(&pick, cx);
        // Alone in its group, the tab had no neighbour to show instead, and
        // hiding it would leave it on screen to close under the user.
        // From here no group shows it, so one that does later was moved to
        // it: by the user, or by another client's focus.
        if self.group_shows(&pick) {
            return false;
        }
        self.browser.hidden.hide(pending.key(), [pick]);
        self.browser
            .undo
            .pending
            .retain(|old| old.scope != pending.scope || old.closed != *closed);
        self.browser.undo.pending.push(pending);
        self.wake_undo_close(cx);
        cx.notify();
        true
    }

    /// Lists again the newest tab of the focused workspace whose close is
    /// waiting, and shows it. Returns whether there was one.
    pub(crate) fn undo_tab_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(key) = self.browser_key() else {
            return false;
        };
        let Some(index) = self
            .browser
            .undo
            .pending
            .iter()
            .rposition(|pending| pending.key() == key)
        else {
            return false;
        };
        let pending = self.browser.undo.pending.remove(index);
        let pick = pending.pick();
        self.browser.hidden.show(&key, &pick);
        if let Some(group) = self.ensure_layout().map(|layout| layout.owner(&pick)) {
            self.move_tab_to_group(pick, group, window, cx);
        }
        self.wake_undo_close(cx);
        cx.notify();
        true
    }

    /// Sends the closes that are due and drops those settled, on every tick
    /// and when a wake fires.
    pub(crate) fn poll_undo_close(&mut self, cx: &mut Context<Self>) {
        if self.browser.undo.pending.is_empty() {
            return;
        }
        let now = Instant::now();
        let pending = std::mem::take(&mut self.browser.undo.pending);
        let mut kept = Vec::with_capacity(pending.len());
        for entry in pending {
            match self.settle_undo_close(&entry, now, cx) {
                Outcome::Wait => kept.push(entry),
                Outcome::Done => {}
                Outcome::Restore => self.browser.hidden.show(&entry.key(), &entry.pick()),
            }
        }
        self.browser.undo.pending = kept;
        self.wake_undo_close(cx);
    }

    /// Whether the close of `tab` on the shown host waits, for tests.
    #[cfg(test)]
    pub(crate) fn undo_waits(&self, tab: &str) -> bool {
        let host = scope(&self.endpoints[self.selected_endpoint]);
        self.browser.undo.waits(&host, tab)
    }

    /// Sends every waiting close now, as the app quits or the window goes:
    /// a tab the user closed must not outlive the window that hid it. Only
    /// queued: a close the connection drops as it stops leaves its tab open.
    pub(crate) fn flush_undo_close(&mut self) {
        self.browser.undo.wake = None;
        for entry in std::mem::take(&mut self.browser.undo.pending) {
            let handle = self
                .endpoints
                .iter()
                .find(|endpoint| scope(endpoint) == entry.scope)
                .and_then(|endpoint| endpoint.connection.handle.as_ref());
            if let Some(handle) = handle {
                let params = json!({ "tab_id": entry.closed.tab_id });
                let _ = handle.request(&entry.boot, Method::TabClose, params);
            }
        }
    }

    fn settle_undo_close(
        &mut self,
        entry: &Pending,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let Some(index) = self
            .endpoints
            .iter()
            .position(|endpoint| scope(endpoint) == entry.scope)
        else {
            // The host is gone from the window, and its tab with it.
            return Outcome::Done;
        };
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &self.endpoints[index].live
        };
        let Some(snapshot) = live.snapshot.as_ref() else {
            return Outcome::Wait;
        };
        // The daemon restarted: the tab it hid ended with the old boot, and
        // one listed now under its id is another tab, which must show.
        if snapshot.boot_id != entry.boot {
            return Outcome::Restore;
        }
        let tab = entry.closed.tab_id.as_str();
        if !snapshot.tabs.iter().any(|open| open.tab_id == tab) {
            return Outcome::Done;
        }
        if index == self.selected_endpoint
            && self.browser_key() == Some(entry.key())
            && self.group_shows(&entry.pick())
        {
            return Outcome::Restore;
        }
        if now < entry.due {
            return Outcome::Wait;
        }
        let Some(handle) = &self.endpoints[index].connection.handle else {
            return Outcome::Wait;
        };
        match handle.request(&entry.boot, Method::TabClose, json!({ "tab_id": tab })) {
            Ok(_) => {
                let (scope, closed) = (entry.scope.clone(), entry.closed.clone());
                if index == self.selected_endpoint {
                    self.record_closed_herdr_tab(closed, cx);
                } else {
                    Store::update(cx, |store| store.record_closed_herdr_tab(scope, closed));
                }
                Outcome::Done
            }
            // The queue is momentarily full: the next tick tries again.
            Err(sheprd_client::Error::Full) => Outcome::Wait,
            Err(error) => {
                tracing::warn!(%error, "Could not close a tab after its undo window");
                Outcome::Restore
            }
        }
    }

    /// Wakes the window when the next close is due, which no tick may come
    /// for while the window is idle.
    fn wake_undo_close(&mut self, cx: &mut Context<Self>) {
        let Some(due) = self
            .browser
            .undo
            .pending
            .iter()
            .map(|entry| entry.due)
            .min()
        else {
            self.browser.undo.wake = None;
            return;
        };
        let now = Instant::now();
        if self
            .browser
            .undo
            .wake
            .as_ref()
            .is_some_and(|(set, _)| *set == due && due > now)
        {
            return;
        }
        // Past due, a close is retrying; the wake comes back for it shortly.
        let wait = due.saturating_duration_since(now).max(RETRY);
        let wake = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, cx| this.poll_undo_close(cx));
        });
        self.browser.undo.wake = Some((due, wake));
    }
}

#[cfg(test)]
mod tests;
