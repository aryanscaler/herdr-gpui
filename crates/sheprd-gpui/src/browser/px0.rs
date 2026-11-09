//! Workspace-scoped px0 pages; the workbench owns its checkout processes.
//! A local workspace uses the workbench on this computer, and one on an SSH
//! host that host's own (see [`crate::px0::host`]), which opens the
//! workspace's folder there.
use super::{Editor, GroupId, Location, Store, TabId, WebUrl, view::store};
use crate::i18n::{Localize, fl};
use crate::{
    SheprdWindow,
    px0::{Hosts, Launcher, State},
    window::Flash,
};
use gpui::{prelude::*, *};
use sheprd_client::ConnectTarget;
use std::collections::HashMap;

/// Which workbench serves a workspace's px0 tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Px0Host {
    /// The workbench on this computer, or the configured address.
    Local,
    /// The workbench the app runs on this SSH target.
    Ssh(String),
}

impl Px0Host {
    fn state(&self, cx: &App) -> State {
        match self {
            Self::Local => Launcher::state(cx),
            Self::Ssh(target) => Hosts::state(cx, target),
        }
    }

    fn want(&self, cx: &mut App) {
        match self {
            Self::Local => Launcher::want(cx),
            Self::Ssh(target) => Hosts::want(cx, target),
        }
    }

    fn reset(&self, cx: &mut App) {
        match self {
            Self::Local => Launcher::reset(cx),
            Self::Ssh(target) => Hosts::reset(cx, target),
        }
    }
}

/// The workbench, its address, and the checkout a page was created with.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    host: Px0Host,
    base: WebUrl,
    root: String,
}

#[derive(Default)]
pub(super) struct Pages {
    phase: Option<std::mem::Discriminant<State>>,
    /// What each page was created with. Reopening a tab or changing its
    /// server rebinds it; stale persisted ports are never loaded.
    bound: HashMap<TabId, Binding>,
}

impl SheprdWindow {
    /// The focused workspace's workbench and starting folder on its own
    /// host: this computer's for a local workspace, an SSH host's for one
    /// there. Other remote workspaces have none, nor SSH ones on Windows.
    pub(crate) fn px0_target(&self) -> Option<(Px0Host, String)> {
        let endpoint = self.endpoints.get(self.selected_endpoint)?;
        let (host, root) = match &endpoint.connection.target {
            // Forwarding the host's port needs `ssh -L`, which runs only on Unix.
            ConnectTarget::Ssh { target, .. } if cfg!(unix) => {
                (Px0Host::Ssh(target.clone()), self.host_editor_root()?)
            }
            _ => (Px0Host::Local, self.local_editor_root()?),
        };
        (root.len() <= 4096 && !root.chars().any(char::is_control))
            .then(|| (host, root.into_owned()))
    }

    pub(crate) fn px0_root(&self) -> Option<String> {
        self.px0_target().map(|(_, root)| root)
    }

    pub(crate) fn open_px0(
        &mut self,
        group: Option<GroupId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<TabId> {
        if !super::EMBEDDED {
            self.show_flash(Flash::warning(fl!("browser-px0-unsupported")), cx);
            return None;
        }
        if self.px0_root().is_none() {
            self.show_flash(Flash::warning(fl!("browser-px0-no-workspace")), cx);
            return None;
        }
        self.open_embedded_editor(Editor::Px0, group, None, window, cx)
    }

    pub(super) fn ensure_px0_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !super::EMBEDDED {
            return;
        }
        let focused = self.px0_target();
        let state = focused
            .as_ref()
            .map_or_else(|| Launcher::state(cx), |(host, _)| host.state(cx));
        let phase = Some(std::mem::discriminant(&state));
        if self.browser.px0.phase != phase {
            self.browser.px0.phase = phase;
            cx.notify();
        }
        // A workbench starting again has a new address, so its pages close;
        // one that failed keeps them, and the edits in them.
        self.unbind_px0_pages(|binding| {
            matches!(binding.host.state(cx), State::Idle | State::Starting)
        });
        self.browser
            .px0
            .bound
            .retain(|id, _| store(cx).is_some_and(|store| store.get(*id).is_some()));
        let tab = self
            .browser_key()
            .and_then(|(scope, workspace)| store(cx)?.px0_tab(&scope, &workspace).cloned());
        let Some(mut tab) = tab.filter(|tab| self.group_shows_page(tab.id, cx)) else {
            return;
        };
        let Some((host, root)) = focused else {
            return;
        };
        host.want(cx);
        let State::Ready(base) = host.state(cx) else {
            return;
        };
        let binding = Binding { host, base, root };
        if self.browser.px0.bound.get(&tab.id) != Some(&binding) {
            let url = match crate::px0::workspace_url(&binding.base, &binding.root) {
                Ok(url) => url,
                Err(error) => {
                    self.browser.failed.insert(tab.id, error.localize().into());
                    return;
                }
            };
            #[cfg(any(target_os = "macos", windows))]
            self.browser.pages.close(tab.id);
            self.browser.failed.remove(&tab.id);
            tab.location = Some(Location::Web { url });
            Store::update(cx, |store| {
                store.visited(tab.id, tab.location.clone(), Some("px0"))
            });
            self.browser.px0.bound.insert(tab.id, binding);
        }
        self.create_editor_page(&tab, window, cx);
    }

    /// Starts `host`'s workbench again on the next ensure. Every page on the
    /// host, not only the focused workspace's, is at the old address.
    pub(super) fn restart_px0(&mut self, host: &Px0Host, cx: &mut App) {
        host.reset(cx);
        self.unbind_px0_pages(|binding| binding.host == *host);
    }

    /// The tabs bound to a workbench, for the SSH test, which runs on Unix.
    #[cfg(all(test, unix))]
    pub(super) fn px0_bound_tabs(&self) -> Vec<TabId> {
        self.browser.px0.bound.keys().copied().collect()
    }

    /// Closes the pages whose binding is `stale` and forgets the bindings.
    fn unbind_px0_pages(&mut self, stale: impl Fn(&Binding) -> bool) {
        let ids: Vec<TabId> = self
            .browser
            .px0
            .bound
            .iter()
            .filter(|(_, binding)| stale(binding))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            #[cfg(any(target_os = "macos", windows))]
            self.browser.pages.close(id);
            self.browser.failed.remove(&id);
            self.browser.px0.bound.remove(&id);
        }
    }

    pub(super) fn render_px0_status(
        &self,
        failure: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let text = if let Some(failure) = failure {
            failure.to_string()
        } else if !super::EMBEDDED {
            fl!("browser-px0-unsupported")
        } else if let Some((host, _)) = self.px0_target() {
            match host.state(cx) {
                State::Idle | State::Starting => fl!("browser-px0-starting"),
                State::Ready(_) => fl!("browser-px0-loading"),
                State::Failed(error) => error.localize(),
            }
        } else {
            fl!("browser-px0-no-workspace")
        };
        super::code_view::centered(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .items_center()
                .debug_selector(|| "px0-placeholder".into())
                .child(text)
                .child(
                    div()
                        .id("px0-retry")
                        .cursor_pointer()
                        .child(fl!("browser-px0-retry"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some((host, _)) = this.px0_target() {
                                this.restart_px0(&host, cx);
                            }
                            this.ensure_px0_page(window, cx);
                            cx.notify();
                        })),
                ),
        )
    }
}
