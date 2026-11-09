//! Which core a herdr-core tab's terminals run in: this computer's, or the
//! one on the saved host its workspace belongs to. Terminal IDs are only
//! unique within one core, so each host keeps its own hub and its own list
//! of what that core reports.

use super::{CoreTerminals, Error, Hub, Purpose, State, infos::Infos};
use crate::{
    SheprdWindow,
    browser::{Scope, TabId},
    controls::Command,
    i18n::fl,
    window::Flash,
};
use gpui::{Context, Window};
use sheprd_backend::{TerminalId, TerminalInfo};
use sheprd_client::ConnectTarget;
use sheprd_core_client::CoreBackend;
use std::{collections::VecDeque, time::Instant};

/// The core a tab's terminals run in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Host {
    /// This computer's core.
    Local,
    /// The core on the saved host this scope names.
    Remote(Scope),
}

impl Host {
    /// The device the core runs on, as notification mutes name it.
    pub(crate) fn endpoint(&self) -> &str {
        match self {
            Self::Local => crate::endpoint::LOCAL,
            Self::Remote(scope) => scope.as_str(),
        }
    }

    /// The core a workspace in `scope` starts its terminals in.
    pub(crate) fn of(scope: &Scope) -> Self {
        if scope.is_local() {
            Self::Local
        } else {
            Self::Remote(scope.clone())
        }
    }
}

/// Another host's core and what it reports of its terminals.
pub(super) struct Remote {
    pub(super) hub: Hub,
    pub(super) infos: Infos,
    /// The saved device the hub dials, to tell when it was edited.
    pub(super) target: ConnectTarget,
    /// Browser request labels, by terminal, this window never serves (again):
    /// those already there when it first listed the core, those left on a
    /// terminal it did not show, and those it served and is removing.
    /// `None` until the first list.
    pub(super) old_requests: Option<std::collections::HashSet<(TerminalId, String)>>,
}

impl CoreTerminals {
    /// The hub of `host`'s core, once the window has asked for it.
    pub(crate) fn hub_of(&self, host: &Host) -> Option<&Hub> {
        match host {
            Host::Local => Some(&self.hub),
            Host::Remote(scope) => self.remote.get(scope).map(|remote| &remote.hub),
        }
    }

    pub(super) fn hub_of_mut(&mut self, host: &Host) -> Option<&mut Hub> {
        match host {
            Host::Local => Some(&mut self.hub),
            Host::Remote(scope) => self.remote.get_mut(scope).map(|remote| &mut remote.hub),
        }
    }

    pub(super) fn backend_of(&self, host: &Host) -> Option<&CoreBackend> {
        self.hub_of(host)?.backend()
    }

    /// Whether `host`'s core is connected now.
    pub(crate) fn answers(&self, host: &Host) -> bool {
        self.backend_of(host).is_some()
    }

    pub(super) fn infos_of(&self, host: &Host) -> Option<&Infos> {
        match host {
            Host::Local => Some(&self.infos),
            Host::Remote(scope) => self.remote.get(scope).map(|remote| &remote.infos),
        }
    }

    pub(super) fn infos_of_mut(&mut self, host: &Host) -> Option<&mut Infos> {
        match host {
            Host::Local => Some(&mut self.infos),
            Host::Remote(scope) => self.remote.get_mut(scope).map(|remote| &mut remote.infos),
        }
    }

    /// The core `tab`'s terminals run in; this computer's for a tab the
    /// window does not show.
    pub(crate) fn host_of(&self, tab: TabId) -> Host {
        self.tabs
            .get(&tab)
            .map_or(Host::Local, |shown| shown.host.clone())
    }

    /// What `tab`'s core reports of `terminal`.
    pub(crate) fn info(&self, tab: TabId, terminal: &TerminalId) -> Option<&TerminalInfo> {
        self.infos_of(&self.tabs.get(&tab)?.host)?.get(terminal)
    }

    /// Every host whose core the window knows of, this computer first.
    pub(super) fn hosts(&self) -> Vec<Host> {
        std::iter::once(Host::Local)
            .chain(self.remote.keys().cloned().map(Host::Remote))
            .collect()
    }

    /// Uses `hub` for the core of the host `scope` names, as the first ask
    /// for it would, so a test never dials a real host.
    #[cfg(test)]
    pub(crate) fn set_remote_hub_for_test(
        &mut self,
        scope: Scope,
        target: ConnectTarget,
        hub: Hub,
    ) {
        self.remote.insert(
            scope,
            Remote {
                hub,
                infos: Infos::default(),
                old_requests: None,
                target,
            },
        );
    }

    /// `host`'s hub, which a test drives directly.
    // Only the lifecycle tests, which need Unix sockets, use it.
    #[cfg(all(test, unix))]
    pub(crate) fn hub_of_mut_for_test(&mut self, host: &Host) -> Option<&mut Hub> {
        self.hub_of_mut(host)
    }

    /// Whether a terminal waits for `host`'s core to connect.
    // Only the lifecycle tests, which need Unix sockets, use it.
    #[cfg(all(test, unix))]
    pub(crate) fn waits_on_for_test(&self, host: &Host) -> bool {
        self.waits_on(host)
    }

    /// Records `info` as what `host`'s core reports, as a finished list
    /// would.
    // Only the lifecycle tests, which need Unix sockets, use it.
    #[cfg(all(test, unix))]
    pub(crate) fn record_for_test(&mut self, host: &Host, info: TerminalInfo) {
        if let Some(infos) = self.infos_of_mut(host) {
            infos.insert(info);
        }
    }

    /// Shows `terminal` of `host`'s core in `tab`, as a restored tab would.
    // Only the lifecycle tests, which need Unix sockets, use it.
    #[cfg(all(test, unix))]
    pub(crate) fn show_on_host_for_test(&mut self, tab: TabId, host: Host, terminal: TerminalId) {
        let panes = super::panes::Panes::single(terminal);
        self.tabs
            .insert(tab, super::TabPanes::new(host, panes, false));
    }

    /// A terminal for `purpose` on `host` that started as `outcome` says,
    /// waiting to be placed.
    // Only the lifecycle tests, which need Unix sockets, use it.
    #[cfg(all(test, unix))]
    pub(crate) fn start_for_test(
        &mut self,
        purpose: Purpose,
        host: Host,
        outcome: Result<TerminalInfo, sheprd_core_client::Error>,
    ) {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let _ = send.send(outcome);
        self.starting = Some((purpose, host, receive));
    }

    /// Whether a started terminal waits to be placed.
    // Only the lifecycle tests, which need Unix sockets, use it.
    #[cfg(all(test, unix))]
    pub(crate) fn is_starting(&self) -> bool {
        self.starting.is_some()
    }

    /// Reads what each core reports while any of its tabs is known, for
    /// split chrome and the sidebar's rows. True when what is known changed.
    pub(super) fn poll_infos(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for host in self.hosts() {
            // This computer's core is read while it answers, tabs or not:
            // a phone may start a terminal there for one of its workspaces
            // (`adopt`), and its stand-in shell runs agents too.
            let listed = host == Host::Local || self.tabs.values().any(|shown| shown.host == host);
            let backend = listed.then(|| self.backend_of(&host).cloned()).flatten();
            if let Some(infos) = self.infos_of_mut(&host) {
                changed |= infos.poll(backend.as_ref(), now);
            }
        }
        changed
    }

    /// Drops another host's hub, its views, and a terminal wanted or
    /// starting there; the next ask for the host dials it anew. A terminal
    /// that was starting keeps running on the machine it started on, but is
    /// never placed in a tab, which would attach its ID on another machine.
    /// This computer's hub stays.
    pub(super) fn forget_host(&mut self, host: &Host) {
        let Host::Remote(scope) = host else {
            return;
        };
        self.remote.remove(scope);
        let tabs = &self.tabs;
        self.views
            .retain(|(tab, _), _| tabs.get(tab).is_none_or(|shown| shown.host != *host));
        self.wanted.retain(|(_, wanted, _, _)| wanted != host);
        if self
            .starting
            .as_ref()
            .is_some_and(|(_, starting, _)| starting == host)
        {
            self.starting = None;
        }
    }

    /// Whether a terminal waits for `host`'s core to connect.
    pub(super) fn waits_on(&self, host: &Host) -> bool {
        self.wanted.iter().any(|(_, wanted, ..)| wanted == host)
    }

    /// How many agents `host`'s core last reported working, for ranking
    /// hosts by how busy they are. The list is only kept current while the
    /// window has a tab of that core, so without one it counts none rather
    /// than agents that may have stopped since.
    pub(crate) fn working(&self, host: &Host) -> u32 {
        if !self.tabs.values().any(|shown| shown.host == *host) {
            return 0;
        }
        self.infos_of(host).map_or(0, Infos::working)
    }

    /// Whether `host` was found to have no herdr-core installed, so new
    /// tabs there stay with its Herdr.
    pub(crate) fn lacks_core(&self, host: &Host) -> bool {
        matches!(
            self.hub_of(host).map(Hub::state),
            Some(State::Unavailable(error)) if error.core_missing()
        )
    }
}

impl SheprdWindow {
    /// Takes each hub's outcome. A lost connection ended the workers of its
    /// host's panes: their views stay with their last frame dimmed, and drop
    /// what is typed until that core answers again and each terminal is
    /// attached anew; nothing is started or replayed.
    pub(super) fn poll_core_hubs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = self.forget_stale_core_hosts();
        for host in self.core_terminals.hosts() {
            let Some(hub) = self.core_terminals.hub_of_mut(&host) else {
                continue;
            };
            let connected = hub.backend().is_some();
            if !hub.poll() {
                continue;
            }
            changed = true;
            if connected && self.core_terminals.backend_of(&host).is_none() {
                let terminals = &self.core_terminals;
                let fallback = (host == Host::Local)
                    .then(|| match &terminals.fallback {
                        super::fallback::Fallback::Shown(view) => Some(view.clone()),
                        _ => None,
                    })
                    .flatten();
                let views: Vec<_> = terminals
                    .views
                    .iter()
                    .filter(|((tab, _), _)| terminals.host_of(*tab) == host)
                    .map(|(_, view)| view.clone())
                    .chain(fallback)
                    .collect();
                for view in views {
                    view.update(cx, |view, cx| view.set_stale(cx));
                }
            }
            let failed = match self.core_terminals.hub_of(&host).map(Hub::state) {
                Some(State::Unavailable(error)) => Some(std::sync::Arc::clone(error)),
                _ => None,
            };
            // Every terminal waiting for a core that cannot start is
            // dropped, and the user is told once.
            if let Some(error) = failed {
                let (here, others) =
                    std::mem::take(&mut self.core_terminals.wanted)
                        .into_iter()
                        .partition::<VecDeque<_>, _>(|(_, wanted, _, _)| *wanted == host);
                self.core_terminals.wanted = others;
                if let Some((purpose, ..)) = here.into_iter().next() {
                    self.core_unavailable(purpose, &error, window, cx);
                }
            }
        }
        if changed {
            self.start_core_shell();
            cx.notify();
        }
    }

    /// Tells the user why a terminal wanted for `purpose` did not start. A
    /// new tab on a host without herdr-core is made by the host's Herdr
    /// instead, as it was before Herdr 2.
    fn core_unavailable(
        &mut self,
        purpose: Purpose,
        error: &Error,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A workspace the app keeps has no Herdr to make the tab instead:
        // one left empty goes, and the host keeps new workspaces in its
        // Herdr from now on, as it lacks a core.
        if error.core_missing()
            && let Purpose::Tab { scope, workspace } = &purpose
            && crate::browser::is_kept_id(workspace)
        {
            self.forget_empty_kept_workspace(scope, workspace, window, cx);
            self.show_flash(
                Flash::warning(fl!("core-terminals-workspaces-need-core")),
                cx,
            );
            return;
        }
        // Only while the workspace the tab was asked for is still the one
        // shown, since the Herdr tab opens in the focused workspace.
        if error.core_missing()
            && let Purpose::Tab { scope, workspace } = &purpose
            && self
                .browser_key()
                .is_some_and(|(focused, shown)| focused == *scope && shown == *workspace)
        {
            self.command(Command::Tab, window, cx);
            self.show_flash(Flash::hint(fl!("core-terminals-host-without-core")), cx);
            return;
        }
        let reason = error.to_string();
        self.show_flash(
            Flash::warning(fl!("core-terminals-unavailable", reason = reason)),
            cx,
        );
    }

    /// The hub of `host`'s core, made the first time a remote host is asked
    /// for; `None` for a host that is no longer a saved device.
    pub(super) fn core_hub(&mut self, host: &Host) -> Option<&mut Hub> {
        if let Host::Remote(scope) = host
            && !self.core_terminals.remote.contains_key(scope)
        {
            let target = self.saved_target(scope)?.clone();
            self.core_terminals.remote.insert(
                scope.clone(),
                Remote {
                    hub: Hub::remote(target.clone()),
                    infos: Infos::default(),
                    old_requests: None,
                    target,
                },
            );
        }
        self.core_terminals.hub_of_mut(host)
    }

    /// Where the saved device `scope` names connects now.
    fn saved_target(&self, scope: &Scope) -> Option<&ConnectTarget> {
        self.endpoints
            .iter()
            .find(|endpoint| crate::browser::scope(endpoint) == *scope)
            .map(|endpoint| &endpoint.connection.target)
    }

    /// Drops the hub of every host whose device was removed or now connects
    /// somewhere else, with its views and a terminal wanted there, so no new
    /// terminal starts on a machine the device no longer names. A host still
    /// saved gets a new hub the next time its tabs ask. True when any went.
    fn forget_stale_core_hosts(&mut self) -> bool {
        let stale: Vec<Scope> = self
            .core_terminals
            .remote
            .iter()
            .filter(|(scope, remote)| self.saved_target(scope) != Some(&remote.target))
            .map(|(scope, _)| scope.clone())
            .collect();
        for scope in &stale {
            self.core_terminals
                .forget_host(&Host::Remote(scope.clone()));
        }
        !stale.is_empty()
    }
}
