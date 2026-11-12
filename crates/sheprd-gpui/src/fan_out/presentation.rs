//! Lane presentation is addressed by its captured endpoint, never the host
//! selected when a worker happens to finish.

use crate::{
    SheprdWindow,
    browser::{Location, Scope, Store, scope},
    core_terminals::Host,
    i18n::fl,
    window::Flash,
};
use gpui::{App, Context, Window};
use std::path::Path;

impl SheprdWindow {
    pub(super) fn fan_out_scope(&self, endpoint: &str) -> Option<Scope> {
        self.endpoints
            .iter()
            .find(|known| known.id == endpoint)
            .map(scope)
    }

    pub(super) fn open_fan_out_folder(
        &mut self,
        endpoint: &str,
        host: &crate::teleport::Host,
        label: &str,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.fan_out_host_matches(endpoint, host) || !self.select_endpoint(endpoint, cx) {
            return;
        }
        self.refresh_core_session(cx);
        match self.kept_workspace_at(Path::new(path)) {
            Some(id) => {
                self.focus_kept_workspace(&id, cx);
            }
            None => self.new_kept_workspace(Some(label), Some(path.into()), cx),
        }
    }

    pub(super) fn show_fan_out_terminal(
        &mut self,
        endpoint: &str,
        host: &crate::teleport::Host,
        label: &str,
        path: &str,
        location: Location,
        cx: &mut Context<Self>,
    ) {
        if !self.fan_out_host_matches(endpoint, host) {
            return;
        }
        let Some(scope) = self.fan_out_scope(endpoint) else {
            return;
        };
        // Recovery can report the same terminal again. Reuse both its kept
        // workspace and its tab, rather than opening competing input views.
        let existing = self.kept_workspace_in(&scope, Path::new(path), cx);
        let id = existing
            .clone()
            .or_else(|| self.keep_workspace_in(&scope, Some(label), Some(Path::new(path)), cx));
        let Some(id) = id else { return };
        if already_shown(cx, &scope, &id, &location) {
            return;
        }
        let tab = Store::update(cx, |store| {
            store.open(scope.clone(), &id, Some(location), None)
        });
        if tab.is_none() {
            if existing.is_none() {
                Store::update(cx, |store| store.forget_kept(&scope, &id));
            }
            // A presentation failure does not authorize killing an agent.
            self.show_flash(Flash::warning(fl!("core-terminals-too-many-tabs")), cx);
        }
        self.refresh_core_session(cx);
        self.sync_core_views(cx);
        cx.notify();
    }

    pub(super) fn close_fan_out_folder(
        &mut self,
        checkout: &super::job::Checkout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.fan_out_host_matches(&checkout.endpoint_id, &checkout.host) {
            return;
        }
        let Some(scope) = self.fan_out_scope(&checkout.endpoint_id) else {
            return;
        };
        let Some(id) = self.kept_workspace_in(&scope, Path::new(checkout.path()), cx) else {
            return;
        };
        if checkout.recoverable {
            self.forget_removed_kept_workspace_in(&scope, &id, window, cx);
        } else {
            self.close_kept_workspace_in(&scope, &id, window, cx);
        }
    }

    pub(super) fn fan_out_core_host(&self, endpoint: &str) -> Option<Host> {
        self.fan_out_scope(endpoint).as_ref().map(Host::of)
    }

    pub(super) fn fan_out_host_matches(
        &self,
        endpoint: &str,
        host: &crate::teleport::Host,
    ) -> bool {
        self.endpoints
            .iter()
            .find(|known| known.id == endpoint)
            .and_then(|known| sheprd_remote::work::host_for_kept(&known.connection.target).ok())
            .as_ref()
            == Some(host)
    }
}

fn already_shown(cx: &App, scope: &Scope, workspace: &str, location: &Location) -> bool {
    cx.try_global::<Store>().is_some_and(|store| {
        store
            .in_workspace(scope, workspace)
            .any(|tab| tab.location.as_ref() == Some(location))
    })
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[allow(clippy::unwrap_used)]
mod tests;
