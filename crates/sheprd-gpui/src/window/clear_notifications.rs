//! Clear All Notifications: every window's toasts, shown and waiting, and its
//! Recent alerts, and the OS notifications the app posted, at once.
//!
//! Agents' status is not touched. A finished agent's Done dot and its place
//! on the Dock badge are the daemon's, which acknowledges one when its pane
//! is seen; neither Herdr 1 nor herdr-core offers a way to acknowledge one
//! without showing it, and a client-local seen ledger would make this client
//! disagree with every other (see AGENTS.md). An agent waiting on the user is
//! a question, not a notification, so it stays too.

use super::{SheprdWindow, main_window::MainWindow};
use gpui::{Context, Window};
use std::time::Instant;

impl SheprdWindow {
    /// Clears this window's and every other main window's notifications,
    /// then withdraws what went to the OS notification center.
    pub(crate) fn clear_all_notifications(&mut self, window: &Window, cx: &mut Context<Self>) {
        // One cutoff for every window: a notice two windows received drops
        // from both, even if one has not polled its inbox yet.
        let cutoff = Instant::now();
        self.clear_notifications(cutoff, cx);
        let this = MainWindow::of(window, cx);
        for other in MainWindow::all(cx) {
            if this.as_ref() == Some(&other) {
                continue;
            }
            // A window closing meanwhile has nothing left to clear.
            let _ = other.update(cx, |view, _, cx| view.clear_notifications(cutoff, cx));
        }
        super::system_notifications::withdraw_all(cx);
        crate::core_terminals::forget_pending_posts(cx);
    }

    /// Drops every host's toasts and alerts in this window, and the daemon
    /// notices that arrived by `cutoff` and still wait in an inbox.
    fn clear_notifications(&mut self, cutoff: Instant, cx: &mut Context<Self>) {
        for endpoint in &mut self.endpoints {
            endpoint.toasts.clear(cutoff);
        }
        cx.notify();
    }
}
