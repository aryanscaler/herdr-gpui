//! Muting the notifications of the device, workspace, or tab a menu targets.
//! The mutes live in [`crate::notification_mutes`].
// i18n: migrated

use crate::{
    SheprdWindow,
    notification_mutes::{Daemon, Mutes, Scope},
};
use gpui::{App, Context};
use std::time::SystemTime;

impl SheprdWindow {
    /// Whether the selected endpoint is muted as a whole.
    pub(crate) fn selected_host_muted(&self, cx: &App) -> bool {
        let endpoint = &self.endpoints[self.selected_endpoint];
        Mutes::of(cx).is_some_and(|mutes| mutes.host(&endpoint.id))
    }

    /// Mutes or unmutes the device `id`, connected or not.
    pub(crate) fn set_host_muted(&mut self, id: &str, muted: bool, cx: &mut Context<Self>) {
        if Mutes::update(cx, |mutes| mutes.set_host(id, muted)) {
            cx.notify();
        }
    }

    /// Whether the selected endpoint's workspace or tab `id` is muted on its
    /// own: a tab silent only through its workspace is not.
    pub(crate) fn muted_on_selected(&self, scope: Scope, id: &str, cx: &App) -> bool {
        // A workspace the app keeps for Herdr 2 has no snapshot to look in.
        if scope == Scope::Workspace && self.is_kept_workspace(id) {
            let browser = crate::browser::scope(&self.endpoints[self.selected_endpoint]);
            return Mutes::of(cx).is_some_and(|mutes| mutes.kept(browser.as_str(), scope, id));
        }
        let Some(snapshot) = self.live.snapshot.as_deref() else {
            return false;
        };
        let daemon = Daemon::of(&self.endpoints[self.selected_endpoint]);
        Mutes::of(cx).is_some_and(|mutes| {
            let silenced = mutes.silenced(&daemon, snapshot);
            match scope {
                Scope::Workspace => silenced.workspace(id),
                Scope::Tab => silenced.own_tab(id),
            }
        })
    }

    /// Mutes or unmutes the selected endpoint's workspace or tab `id`, while
    /// the menu still targets the connection and daemon process `boot` it
    /// opened on; a workspace the app keeps needs no daemon process. Returns
    /// whether anything changed.
    pub(crate) fn set_muted_on_selected(
        &mut self,
        scope: Scope,
        id: &str,
        boot: &str,
        muted: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.menu_target_current() {
            return false;
        }
        if scope == Scope::Workspace && self.is_kept_workspace(id) {
            let browser = crate::browser::scope(&self.endpoints[self.selected_endpoint]);
            let changed =
                crate::core_terminals::mutes::set_kept(cx, browser.as_str(), scope, id, muted);
            if changed {
                cx.notify();
            }
            return changed;
        }
        let Some(snapshot) = self
            .live
            .snapshot
            .clone()
            .filter(|snapshot| snapshot.boot_id == boot)
        else {
            return false;
        };
        let daemon = Daemon::of(&self.endpoints[self.selected_endpoint]);
        let changed = Mutes::update(cx, |mutes| {
            mutes.set(&daemon, &snapshot, scope, id, muted, SystemTime::now())
        });
        if changed {
            cx.notify();
        }
        changed
    }
}
