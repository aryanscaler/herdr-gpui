//! Notification mutes of Herdr 2 tabs. A tab is silent when its device is
//! muted, when it is muted on its own, or when its workspace is: one the app
//! keeps, muted through the app's own mutes, or a Herdr 1 workspace the tab
//! sits in, muted as Herdr 1's tabs are. A silent tab rings no bell, plays
//! no sound, and its agents leave the Dock badge; its marks still show what
//! its agents do. Its panes follow it.

use super::Host;
use crate::{
    SheprdWindow,
    browser::{Location, Store, Tab, TabId, is_kept_id},
    notification_mutes::{Daemon, Mutes, Scope},
};
use gpui::{App, Context};
use sheprd_backend::TerminalId;
use std::time::SystemTime;

/// The mute row a herdr-core tab's menu offers, as a Herdr 1 tab's does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MuteRow {
    Mute,
    Unmute,
    /// The tab is silent because its workspace is muted: lift that.
    UnmuteWorkspace,
    /// The tab is silent because its device is muted: lift that.
    UnmuteDevice,
}

impl SheprdWindow {
    /// Whether the store tab `tab` is silent through its device, its own
    /// mute, or its workspace's.
    pub(crate) fn core_tab_silenced(&self, tab: TabId, cx: &App) -> bool {
        let Some(mutes) = Mutes::any(cx) else {
            return false;
        };
        let Some(tab) = cx.try_global::<Store>().and_then(|store| store.get(tab)) else {
            return false;
        };
        mutes.host(Host::of(&tab.scope).endpoint())
            || mutes.kept(tab.scope.as_str(), Scope::Tab, &tab.id.to_string())
            || self.core_workspace_muted(tab, cx)
    }

    /// Whether `terminal` on `host`'s core is silent: the device is muted,
    /// or a tab that drives it is. The shell shown in place of Herdr 1's
    /// follows the workspace it stands in.
    pub(crate) fn core_terminal_silenced(
        &self,
        host: &Host,
        terminal: &TerminalId,
        cx: &App,
    ) -> bool {
        let Some(mutes) = Mutes::any(cx) else {
            return false;
        };
        if mutes.host(host.endpoint()) {
            return true;
        }
        if *host == Host::Local
            && self
                .core_fallback_view()
                .is_some_and(|view| view.read(cx).terminal() == terminal)
            && self.core_fallback_silenced(cx)
        {
            return true;
        }
        let Some(store) = cx.try_global::<Store>() else {
            return false;
        };
        store
            .tabs()
            .filter(|tab| Host::of(&tab.scope) == *host)
            .filter(|tab| {
                matches!(
                    &tab.location,
                    Some(Location::CoreTerminal { panes, observe: false, .. })
                        if panes.contains(terminal)
                )
            })
            .any(|tab| self.core_tab_silenced(tab.id, cx))
    }

    /// Whether the shell shown in place of Herdr 1's is silent: this
    /// computer is muted, or the workspace it stands in, which the app
    /// keeps, is.
    pub(crate) fn core_fallback_silenced(&self, cx: &App) -> bool {
        let Some(mutes) = Mutes::any(cx) else {
            return false;
        };
        if mutes.host(Host::Local.endpoint()) {
            return true;
        }
        self.core_host().is_some_and(|index| {
            let scope = crate::browser::scope(&self.endpoints[index]);
            mutes.kept(scope.as_str(), Scope::Workspace, super::WORKSPACE)
        })
    }

    /// Whether the workspace `tab` sits in is muted itself.
    fn core_workspace_muted(&self, tab: &Tab, cx: &App) -> bool {
        let Some(mutes) = Mutes::any(cx) else {
            return false;
        };
        if is_kept_id(&tab.workspace_id) {
            return mutes.kept(tab.scope.as_str(), Scope::Workspace, &tab.workspace_id);
        }
        // A Herdr 1 workspace: muted as its snapshot shows it.
        let Some(index) = self
            .endpoints
            .iter()
            .position(|endpoint| crate::browser::scope(endpoint) == tab.scope)
        else {
            return false;
        };
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &self.endpoints[index].live
        };
        live.snapshot.as_deref().is_some_and(|snapshot| {
            mutes
                .silenced(&Daemon::of(&self.endpoints[index]), snapshot)
                .workspace(&tab.workspace_id)
        })
    }

    /// The mute row `tab`'s menu offers: what silences it, the device first,
    /// else muting it.
    pub(crate) fn core_mute_row(&self, tab: TabId, cx: &App) -> MuteRow {
        let Some(stored) = cx.try_global::<Store>().and_then(|store| store.get(tab)) else {
            return MuteRow::Mute;
        };
        let Some(mutes) = Mutes::of(cx) else {
            return MuteRow::Mute;
        };
        if mutes.host(Host::of(&stored.scope).endpoint()) {
            MuteRow::UnmuteDevice
        } else if self.core_workspace_muted(stored, cx) {
            MuteRow::UnmuteWorkspace
        } else if mutes.kept(stored.scope.as_str(), Scope::Tab, &tab.to_string()) {
            MuteRow::Unmute
        } else {
            MuteRow::Mute
        }
    }

    /// Carries out `row` of `tab`'s menu.
    pub(crate) fn apply_core_mute_row(&mut self, tab: TabId, row: MuteRow, cx: &mut Context<Self>) {
        let Some(stored) = cx.try_global::<Store>().and_then(|store| store.get(tab)) else {
            return;
        };
        let (scope, workspace) = (stored.scope.clone(), stored.workspace_id.clone());
        let changed = match row {
            MuteRow::Mute | MuteRow::Unmute => set_kept(
                cx,
                scope.as_str(),
                Scope::Tab,
                &tab.to_string(),
                row == MuteRow::Mute,
            ),
            MuteRow::UnmuteWorkspace if is_kept_id(&workspace) => {
                set_kept(cx, scope.as_str(), Scope::Workspace, &workspace, false)
            }
            MuteRow::UnmuteWorkspace => self.unmute_herdr_workspace(&scope, &workspace, cx),
            MuteRow::UnmuteDevice => Mutes::update(cx, |mutes| {
                mutes.set_host(Host::of(&scope).endpoint(), false)
            }),
        };
        if changed {
            cx.notify();
        }
    }

    /// Lifts the mute of the Herdr 1 workspace `workspace` in `scope`.
    fn unmute_herdr_workspace(
        &self,
        scope: &crate::browser::Scope,
        workspace: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self
            .endpoints
            .iter()
            .position(|endpoint| crate::browser::scope(endpoint) == *scope)
        else {
            return false;
        };
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &self.endpoints[index].live
        };
        let Some(snapshot) = live.snapshot.clone() else {
            return false;
        };
        let daemon = Daemon::of(&self.endpoints[index]);
        Mutes::update(cx, |mutes| {
            mutes.set(
                &daemon,
                &snapshot,
                Scope::Workspace,
                workspace,
                false,
                SystemTime::now(),
            )
        })
    }
}

/// Mutes or unmutes the app's workspace or tab `id` in `scope`, forgetting
/// the mutes of what the app no longer keeps.
pub(crate) fn set_kept(cx: &mut App, scope: &str, kind: Scope, id: &str, muted: bool) -> bool {
    let kept: std::collections::HashSet<(String, Scope, String)> = cx
        .try_global::<Store>()
        .map(|store| {
            let tabs = store.tabs().map(|tab| {
                (
                    tab.scope.as_str().to_owned(),
                    Scope::Tab,
                    tab.id.to_string(),
                )
            });
            let workspaces = store
                .kept_ids()
                .map(|(scope, id)| (scope.to_owned(), Scope::Workspace, id.to_owned()));
            tabs.chain(workspaces).collect()
        })
        .unwrap_or_default();
    Mutes::update(cx, |mutes| {
        mutes.set_kept(
            scope,
            kind,
            id,
            muted,
            SystemTime::now(),
            |scope, kind, id| kept.contains(&(scope.to_owned(), kind, id.to_owned())),
        )
    })
}
