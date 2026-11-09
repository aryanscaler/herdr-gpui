//! Each herdr-core tab's panes as the window keeps them: a view per pane,
//! the pane in use, panes whose program ended, and the layout saved with the
//! tab once a change settles.

use super::{
    CoreTerminalView, Host, Start, TabPanes,
    panes::{Axis, Panes},
    worker,
};
use crate::{
    SheprdWindow,
    browser::{Location, Store, TabId},
    i18n::fl,
    window::Flash,
};
use gpui::{prelude::*, *};
use sheprd_backend::{Backend as _, Role, Size, TerminalId};

/// The size a terminal starts at, before its pane is laid out.
const FIRST_SIZE: (u16, u16) = (80, 24);

/// Kills that did not reach their core, kept for the whole app rather than
/// one window: a pane closed while disconnected leaves its window's saved
/// layout and the shared ending queue, so if its window closes before the
/// core answers, only another window holding the same list can end it.
#[derive(Clone, Default)]
struct Unended(std::sync::Arc<std::sync::Mutex<Vec<crate::browser::Ending>>>);

impl Global for Unended {}

impl super::CoreTerminals {
    /// A window's herdr-core state, sharing the app's unended kills.
    pub(crate) fn shared(cx: &mut App) -> Self {
        Self {
            unended: std::sync::Arc::clone(&cx.default_global::<Unended>().0),
            ..Self::default()
        }
    }
}

impl SheprdWindow {
    /// Gives every pane of the focused workspace's herdr-core tabs a view,
    /// once the hub is connected, and drops the views of closed tabs and
    /// panes.
    pub(super) fn sync_core_views(&mut self, cx: &mut Context<Self>) {
        let Some(store) = cx.try_global::<Store>() else {
            self.core_terminals.views.clear();
            self.core_terminals.tabs.clear();
            return;
        };
        let terminals = &mut self.core_terminals;
        // A tab closed before its layout was saved holds panes its saved
        // layout does not list yet: they end with it too.
        let mut closed = Vec::new();
        terminals.tabs.retain(|id, shown| {
            let open = store.get(*id).is_some();
            if !open && !shown.observe {
                let host = &shown.host;
                closed.extend(
                    (shown.panes.terminals().into_iter())
                        .map(|terminal| (host.clone(), terminal.clone())),
                );
            }
            open
        });
        let tabs = &terminals.tabs;
        terminals.views.retain(|(tab, terminal), _| {
            tabs.get(tab)
                .is_some_and(|shown| shown.panes.contains(terminal))
        });
        if !closed.is_empty() {
            Store::update(cx, |store| store.end_terminals(closed));
        }
        let Some(store) = cx.try_global::<Store>() else {
            return;
        };
        let Some((scope, workspace)) = self.browser_key() else {
            return;
        };
        let mut missing: Vec<(TabId, TerminalId, Role, Host)> = Vec::new();
        let mut overflow = Vec::new();
        let mut emptied = Vec::new();
        for tab in store.in_workspace(&scope, &workspace) {
            let Some(location) = &tab.location else {
                continue;
            };
            let shown = match self.core_terminals.tabs.entry(tab.id) {
                std::collections::hash_map::Entry::Occupied(shown) => shown.into_mut(),
                std::collections::hash_map::Entry::Vacant(place) => {
                    let Some(mut restored) = TabPanes::restore(Host::of(&tab.scope), location)
                    else {
                        continue;
                    };
                    restored.saved = Some(location.clone());
                    place.insert(restored)
                }
            };
            // Each window holds its own copy: one another window saved over
            // is read again, names and all, before this one can save.
            if shown.saved.as_ref() != Some(location) {
                let left = shown.refresh(location);
                overflow.extend(left.overflow);
                if left.emptied {
                    emptied.push(tab.id);
                }
            }
            if shown.workspace.is_none() {
                shown.workspace = Some((scope.clone(), workspace.clone()));
            }
            // Only this computer's folders root a workspace's local views.
            if shown.host == Host::Local {
                shown.keep_root(&self.core_terminals.infos);
            }
            missing.extend(
                shown
                    .panes
                    .terminals()
                    .into_iter()
                    .filter(|terminal| {
                        !self
                            .core_terminals
                            .views
                            .contains_key(&(tab.id, (*terminal).clone()))
                    })
                    .map(|terminal| {
                        let role = if shown.observe {
                            Role::Observer
                        } else {
                            Role::Active
                        };
                        (tab.id, terminal.clone(), role, shown.host.clone())
                    }),
            );
        }
        // Each window closed the other's last pane: the tab goes, as one
        // whose last pane closed does.
        for tab in emptied {
            self.core_terminals.tabs.remove(&tab);
            Store::update(cx, |store| store.close(tab));
        }
        // Panes another window opened in a tab this one filled meanwhile
        // run on in a tab of their own rather than in none.
        for terminal in overflow {
            let location = Location::core_terminal(Panes::single(terminal.clone()), false);
            let opened = Store::update(cx, |store| {
                store.open(scope.clone(), &workspace, Some(location), None)
            });
            let said = match opened {
                Some(_) => fl!("core-terminals-pane-own-tab"),
                None => {
                    tracing::warn!(%terminal, "No room for a herdr-core pane's own tab");
                    fl!("core-terminals-too-many-tabs")
                }
            };
            self.show_flash(Flash::warning(said), cx);
        }
        if missing.is_empty() {
            return;
        }
        let look = self.core_look();
        for (tab, terminal, role, host) in missing {
            // Restored tabs join a running core; only a command starts one
            // here, while another host's bridge starts its own.
            let Some(backend) = self.core_terminals.backend_of(&host).cloned() else {
                if let Some(hub) = self.core_hub(&host) {
                    hub.connect(Start::Join);
                }
                continue;
            };
            let Ok(size) = Size::new(FIRST_SIZE.0, FIRST_SIZE.1) else {
                continue;
            };
            let link = match worker::spawn(backend.clone(), terminal.clone(), size, role) {
                Ok(link) => link,
                Err(error) => {
                    tracing::warn!(%error, "Could not start a herdr-core terminal worker");
                    continue;
                }
            };
            let painter = self.painter.clone();
            let look = look.clone();
            let observe = role == Role::Observer;
            let to_window = super::asks::window_call(cx);
            let view = cx.new(|cx| {
                CoreTerminalView::new(terminal.clone(), link, look, painter, observe, cx)
                    .with_window(to_window)
            });
            self.core_terminals.views.insert((tab, terminal), view);
        }
        // The tab drew its placeholder; it shows the new views now.
        cx.notify();
    }

    /// Attaches the terminals of views left stale by a dropped connection
    /// once the core answers again, joining it meanwhile.
    pub(super) fn relink_stale_core_views(&mut self, cx: &mut Context<Self>) {
        // The stand-in shell has no tab and always types.
        let fallback = self.core_fallback_view().map(|view| (None, view));
        let stale: Vec<(Option<TabId>, Entity<CoreTerminalView>)> = self
            .core_terminals
            .views
            .iter()
            .map(|((tab, _), view)| (Some(*tab), view.clone()))
            .chain(fallback)
            .filter(|(_, view)| view.read(cx).is_stale())
            .collect();
        for (tab, view) in stale {
            let host = tab.map_or(Host::Local, |tab| self.core_terminals.host_of(tab));
            let Some(backend) = self.core_terminals.backend_of(&host).cloned() else {
                if let Some(hub) = self.core_hub(&host) {
                    hub.connect(Start::Join);
                }
                continue;
            };
            let observe = tab
                .and_then(|tab| self.core_terminals.tabs.get(&tab))
                .is_some_and(|shown| shown.observe);
            let role = if observe {
                Role::Observer
            } else {
                Role::Active
            };
            let terminal = view.read(cx).terminal().clone();
            let size = view
                .read(cx)
                .size()
                .or_else(|| Size::new(FIRST_SIZE.0, FIRST_SIZE.1).ok());
            let Some(size) = size else {
                continue;
            };
            match worker::spawn(backend.clone(), terminal, size, role) {
                Ok(link) => view.update(cx, |view, cx| view.relink(link, cx)),
                Err(error) => {
                    tracing::warn!(%error, "Could not start a herdr-core terminal worker")
                }
            }
        }
    }

    /// Takes every pane's new screen, follows the pane the keyboard moved
    /// to, closes panes whose program ended in a split, and titles each tab
    /// after its pane in use, as a Herdr tab is titled after its program.
    pub(super) fn poll_core_panes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((tab, terminal)) = self.core_terminals.focused_pane(window, cx)
            && let Some(shown) = self.core_terminals.tabs.get_mut(&tab)
            && shown.activate(terminal)
        {
            cx.notify();
        }
        let views: Vec<((TabId, TerminalId), Entity<CoreTerminalView>)> = self
            .core_terminals
            .views
            .iter()
            .map(|(key, view)| (key.clone(), view.clone()))
            .collect();
        let in_sight: std::collections::HashSet<TabId> = self
            .browser_key()
            .zip(cx.try_global::<Store>())
            .map(|((scope, workspace), store)| {
                store
                    .in_workspace(&scope, &workspace)
                    .map(|tab| tab.id)
                    .collect()
            })
            .unwrap_or_default();
        // Links are read against each pane's folder, and a path is no link
        // in another host's pane: both are known before the screens change.
        for ((tab, terminal), view) in &views {
            let remote = self.core_terminals.host_of(*tab) != Host::Local;
            let cwd = self
                .core_terminals
                .info(*tab, terminal)
                .and_then(|info| info.cwd.as_deref());
            view.update(cx, |view, cx| {
                view.set_remote(remote, cx);
                view.set_cwd(cwd, cx);
            });
        }
        let polled = views
            .iter()
            .map(|((tab, _), view)| {
                let host = in_sight
                    .contains(tab)
                    .then(|| self.core_terminals.host_of(*tab));
                (view.clone(), host)
            })
            .collect::<Vec<_>>();
        self.poll_core_views(polled, window, cx);
        let mut ended = Vec::new();
        for ((tab, terminal), view) in views {
            let split = self
                .core_terminals
                .tabs
                .get(&tab)
                .is_some_and(|shown| shown.panes.count() > 1);
            if split && view.read(cx).ended() {
                ended.push((tab, terminal));
            }
        }
        for (tab, terminal) in ended {
            self.remove_core_pane(tab, &terminal, window, cx);
        }
        let titles: Vec<(TabId, String)> = self
            .core_terminals
            .tabs
            .iter()
            .filter_map(|(tab, shown)| {
                let title = self.core_tab_title(*tab, shown, cx);
                // A pane's name left over from when it was in use goes, even
                // before the pane now in use has a title of its own.
                let shown_title = store_title(*tab, cx);
                let stale = shown_title
                    .is_some_and(|title| shown.names.values().any(|name| *name == title));
                let title = title.or_else(|| stale.then(|| shown.location().default_title()))?;
                Some((*tab, title))
            })
            .collect();
        for (tab, title) in titles {
            let stale = cx
                .try_global::<Store>()
                .and_then(|store| store.get(tab))
                .is_some_and(|shown| shown.title != title);
            if stale {
                Store::update(cx, |store| store.visited(tab, None, Some(&title)));
            }
        }
    }

    /// What `tab`'s strip shows: the name the user gave the tab, else that
    /// of its pane in use, else the title its program set, if any.
    pub(super) fn core_tab_title(&self, tab: TabId, shown: &TabPanes, cx: &App) -> Option<String> {
        (shown.name.clone())
            .or_else(|| shown.names.get(&shown.active).cloned())
            .or_else(|| {
                let view = self
                    .core_terminals
                    .views
                    .get(&(tab, shown.active.clone()))?;
                view.read(cx).title().map(str::to_owned)
            })
    }

    /// Puts a started terminal of `host`'s core beside `at` in `tab`, and
    /// gives it the keyboard. One that cannot be placed is ended in the core
    /// it started in rather than left behind.
    pub(super) fn place_split(
        &mut self,
        tab: TabId,
        at: &TerminalId,
        axis: Axis,
        (host, new): (Host, TerminalId),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placed = match self.core_terminals.tabs.get_mut(&tab) {
            Some(shown) if shown.host == host => {
                shown.panes.split(at, axis, new.clone()).map(|()| {
                    shown.activate(new.clone());
                    shown.set_zoom(None);
                    shown.unsaved = true;
                })
            }
            _ => Err(super::panes::Error::Missing(at.clone())),
        };
        if let Err(error) = placed {
            self.show_flash(Flash::warning(error.to_string()), cx);
            self.end_core_terminal(&host, new);
            return;
        }
        self.sync_core_views(cx);
        // A split asked for from elsewhere, such as an editor opened from a
        // review, brings its tab forward.
        if self
            .shown_core_pane(cx)
            .is_none_or(|(shown, _)| shown != tab)
        {
            self.show_browser_tab(tab, window, cx);
        }
        self.focus_core_terminal(tab, window, cx);
    }

    /// Takes `terminal` out of `tab`'s panes. The pane beside it, or else
    /// the first, is the tab's pane in use after, and takes the keyboard when
    /// the closed one had it.
    pub(super) fn remove_core_pane(
        &mut self,
        tab: TabId,
        terminal: &TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let had_focus = self
            .core_terminals
            .views
            .get(&(tab, terminal.clone()))
            .is_some_and(|view| view.read(cx).focus_handle().is_focused(window));
        let Some(shown) = self.core_terminals.tabs.get_mut(&tab) else {
            return false;
        };
        let next = [
            super::panes::Direction::Left,
            super::panes::Direction::Up,
            super::panes::Direction::Right,
            super::panes::Direction::Down,
        ]
        .into_iter()
        .find_map(|direction| shown.panes.neighbor(terminal, direction));
        if !shown.panes.remove(terminal) {
            return false;
        }
        if shown.zoomed.as_ref() == Some(terminal) {
            shown.zoomed = None;
            shown.zoom = crate::motion::Toggle::default();
        }
        if &shown.active == terminal {
            shown.active = next.unwrap_or_else(|| shown.panes.first().clone());
        }
        if shown.previous.as_ref() == Some(terminal) {
            shown.previous = None;
        }
        shown.unsaved = true;
        self.core_terminals.views.remove(&(tab, terminal.clone()));
        if had_focus {
            self.focus_core_terminal(tab, window, cx);
        }
        cx.notify();
        true
    }

    /// Ends a terminal of `host`'s core on a worker thread, so herdr-core's
    /// answer is never awaited on the UI thread.
    pub(super) fn end_core_terminal(&self, host: &Host, terminal: TerminalId) {
        let unended = std::sync::Arc::clone(&self.core_terminals.unended);
        // Not connected: it ends once its core answers again.
        let Some(backend) = self.core_terminals.backend_of(host).cloned() else {
            if let Ok(mut unended) = unended.lock() {
                unended.push((host.clone(), terminal));
            }
            return;
        };
        let host = host.clone();
        let ended = std::thread::Builder::new()
            .name("herdr-core-kill".into())
            .spawn(move || {
                let Err(error) = backend.kill(&terminal) else {
                    return;
                };
                tracing::warn!(%error, %terminal, "Could not end a herdr-core terminal");
                // The core's refusal, as for a terminal already gone, is
                // its answer; anything else did not reach it.
                if !matches!(error, sheprd_core_client::Error::Refused { .. })
                    && let Ok(mut unended) = unended.lock()
                {
                    unended.push((host, terminal));
                }
            });
        if let Err(error) = ended {
            tracing::warn!(%error, "Could not start a herdr-core kill worker");
        }
    }

    /// Ends the terminals of closed tabs, joining the core to do it when
    /// the window is not connected yet.
    pub(super) fn end_closed_core_terminals(&mut self, cx: &mut Context<Self>) {
        // Panes closed, or kills sent, while their core did not answer. They
        // wait here rather than with closed tabs, whose queue passes over a
        // terminal a saved layout still holds, as a pane just closed is
        // until its tab saves.
        let unended = match self.core_terminals.unended.lock() {
            Ok(mut unended) => std::mem::take(&mut *unended),
            Err(_) => Vec::new(),
        };
        // A host this window cannot reach yet, as before its saved devices
        // load, keeps its kills for a window that can.
        let mut unreached = Vec::new();
        for (host, terminal) in unended {
            if self.core_terminals.backend_of(&host).is_none()
                && let Some(hub) = self.core_hub(&host)
            {
                hub.connect(Start::Join);
            }
            // Ends it now, or puts it back to wait for the core.
            if self.core_terminals.hub_of(&host).is_some() {
                self.end_core_terminal(&host, terminal);
            } else {
                unreached.push((host, terminal));
            }
        }
        if !unreached.is_empty()
            && let Ok(mut unended) = self.core_terminals.unended.lock()
        {
            unended.extend(unreached);
        }
        if !cx
            .try_global::<Store>()
            .is_some_and(|store| store.has_ending())
        {
            return;
        }
        // A core not reached yet is joined, and its terminals wait for a
        // window that reaches it, this one or another; a failed join is
        // retried after a pause. A host no longer saved cannot be reached,
        // so its terminals are left to it.
        let mut waiting = Vec::new();
        for (host, terminal) in Store::update(cx, Store::take_ending) {
            if self.core_terminals.backend_of(&host).is_some() {
                self.end_core_terminal(&host, terminal);
            } else if let Some(hub) = self.core_hub(&host) {
                hub.connect(Start::Join);
                waiting.push((host, terminal));
            }
        }
        if !waiting.is_empty() {
            Store::update(cx, |store| store.end_terminals(waiting));
        }
    }

    /// Writes changed layouts into their tabs, which saves them.
    pub(super) fn save_core_panes(&mut self, cx: &mut Context<Self>) {
        let changed: Vec<(TabId, Location)> = self
            .core_terminals
            .tabs
            .iter_mut()
            .filter_map(|(tab, shown)| {
                std::mem::take(&mut shown.unsaved).then(|| {
                    let location = shown.location();
                    shown.saved = Some(location.clone());
                    (*tab, location)
                })
            })
            .collect();
        for (tab, location) in changed {
            Store::update(cx, |store| store.visited(tab, Some(location), None));
        }
    }
}

/// The title `tab` shows now.
fn store_title(tab: TabId, cx: &App) -> Option<String> {
    let store = cx.try_global::<Store>()?;
    Some(store.get(tab)?.title.clone())
}
