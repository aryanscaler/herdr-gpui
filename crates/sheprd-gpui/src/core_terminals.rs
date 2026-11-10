//! Terminals that herdr-core (Herdr 2) runs, shown as tabs in the window's
//! groups beside its Herdr tabs and pages. herdr-core streams raw output, so
//! the app emulates each one and paints it with the same painter as a Herdr
//! pane. A tab splits into panes the app lays out itself, drawn in the style
//! of the window's tab layout; any group can show one too, so terminals also
//! sit side by side across groups.
//!
//! Everything that waits runs off the UI thread: the hub connects (and may
//! start the bundled core), each shown pane has a worker that owns its
//! `sheprd_backend::Terminal`, and terminal details are listed on a thread.
//! The window reads their mailboxes on its tick and renders the prepared
//! screens.

mod adopt;
mod asks;
mod attention;
mod checkouts;
mod commands;
mod context;
mod effects;
mod error;
mod fallback;
mod folders;
mod history;
mod hosts;
mod hub;
mod infos;
pub(crate) mod install;
mod layout;
mod marks;
pub(crate) mod menu;
pub(crate) mod moves;
pub(crate) mod mutes;
mod phone;
mod processes;
mod programs;
mod requests;
mod screen;
mod selection;
mod session;
mod sidebar;
mod split;
mod tabs;
mod view;
mod worker;
mod workspaces;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

pub(crate) use commands::acts_on_panes;
pub(crate) use error::Error;
pub(crate) use fallback::Holder;
pub(crate) use hosts::Host;
pub(crate) use hub::{Hub, Start, State};
pub(crate) use sheprd_session::panes;
pub(crate) use view::{CoreTerminalView, LinkClick, Look};
pub(crate) use workspaces::kept_label;

use crate::{
    SheprdWindow,
    browser::{Location, Scope, Store, TabId},
    i18n::fl,
    window::Flash,
};
use gpui::{prelude::*, *};
use layout::merged;
use panes::{Axis, Panes};
use sheprd_backend::{Backend as _, Spawn, TerminalId, TerminalInfo};
use sheprd_client::protocol::AgentStatus;
use std::collections::{HashMap, VecDeque};

/// The workspace of a window with no Herdr 1 installed. herdr-core has no
/// workspaces of its own, so its tabs gather here until the app keeps them.
pub(crate) const WORKSPACE: &str = "herdr-2";

/// The tab the shell shown in place of Herdr 1 counts as in a session: no
/// store tab, whose IDs count up from zero, ever reaches it.
pub(crate) const SHELL_TAB: u64 = u64::MAX;

type Started = std::sync::mpsc::Receiver<Result<TerminalInfo, sheprd_core_client::Error>>;

/// What a terminal being started is for.
#[derive(Clone, Debug)]
pub(crate) enum Purpose {
    /// A new tab in a workspace.
    Tab { scope: Scope, workspace: String },
    /// A new pane beside `at` in a tab.
    Split {
        tab: TabId,
        at: TerminalId,
        axis: Axis,
    },
}

/// What refreshing a tab another window saved leaves for the window to do.
#[derive(Debug, Default, PartialEq)]
struct Refreshed {
    /// Panes the other window opened that the tab had no room left for.
    overflow: Vec<TerminalId>,
    /// Each window closed the other's last pane, so none is left.
    emptied: bool,
}

impl Refreshed {
    /// Whether nothing is left to do.
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A terminal to start, what for, and a file its program takes over once
/// it starts: removed with the request if the start fails or is dropped.
type Wanted = (Purpose, Host, Spawn, Option<tempfile::TempPath>);

/// One tab's panes as the window holds them. The tab's saved location
/// follows, written when a change settles rather than on every drag step.
#[derive(Debug)]
struct TabPanes {
    /// The pane keys go to while the tab has focus.
    active: TerminalId,
    /// The core the tab's terminals run in.
    host: Host,
    /// A mirror tab only watches: it neither types nor splits.
    observe: bool,
    panes: Panes,
    /// The name the user gave the tab, which its program's title does not
    /// replace.
    name: Option<String>,
    /// Names the user gave panes.
    names: std::collections::BTreeMap<TerminalId, String>,
    /// The pane in use before the current one, which Last Pane goes back to.
    previous: Option<TerminalId>,
    /// The folder the tab's first pane was first seen in, kept when its
    /// program moves on, as Herdr keeps where a workspace started.
    root: Option<std::path::PathBuf>,
    /// The tab's location as this window last read or saved it, to tell
    /// when another window saved a change.
    saved: Option<Location>,
    unsaved: bool,
    /// The workspace whose tab this is, known once the window has shown it.
    workspace: Option<(Scope, String)>,
    /// How far the zoomed pane has grown over the tab; it shrinks back to
    /// its place when unzoomed.
    zoom: crate::motion::Toggle,
    /// The pane filling the tab, the rest hidden behind it, kept while it
    /// shrinks back.
    zoomed: Option<TerminalId>,
}

impl TabPanes {
    /// Whether chats are drawn over the tab's panes: it drives its
    /// terminals, and they run in this computer's core. A chat runs
    /// `herdr-acp`, whose program and socket are this computer's, so a tab
    /// on another host shows its terminals only.
    fn holds_chats(&self) -> bool {
        !self.observe && self.host == Host::Local
    }

    fn new(host: Host, panes: Panes, observe: bool) -> Self {
        Self {
            active: panes.first().clone(),
            name: None,
            names: Default::default(),
            host,
            observe,
            panes,
            previous: None,
            root: None,
            saved: None,
            unsaved: false,
            workspace: None,
            zoom: crate::motion::Toggle::default(),
            zoomed: None,
        }
    }

    /// The tab as it was saved: its pane in use, the pane filling it
    /// already zoomed rather than growing as the window opens, and the names
    /// the user gave. A pane the layout no longer holds is ignored.
    fn restore(host: Host, location: &Location) -> Option<Self> {
        let Location::CoreTerminal {
            panes,
            observe,
            active,
            zoomed,
            name,
            names,
        } = location
        else {
            return None;
        };
        let mut shown = Self::new(host, panes.clone(), *observe);
        shown.name.clone_from(name);
        shown.names = names
            .iter()
            .filter(|(terminal, _)| shown.panes.contains(terminal))
            .map(|(terminal, name)| (terminal.clone(), name.clone()))
            .collect();
        if let Some(active) = active
            .as_ref()
            .filter(|active| shown.panes.contains(active))
        {
            shown.active = active.clone();
        }
        if let Some(zoomed) = zoomed
            .as_ref()
            .filter(|zoomed| shown.panes.contains(zoomed))
            && shown.panes.count() > 1
        {
            shown.zoomed = Some(zoomed.clone());
            shown.zoom.set(true, crate::motion::now(), true);
        }
        Some(shown)
    }

    /// Takes `location`, saved by another window since this one last read
    /// or saved the tab, so that its next save does not undo it.
    ///
    /// The layout merges three ways against what this window last read: a
    /// side that did not change it takes the other's; when both did, this
    /// window's layout stays, less the panes the other closed and with the
    /// panes it opened, so a split placed here while another window saved
    /// is kept. The pane in use is this window's, where the keyboard is;
    /// a zoom stands only on that pane, so commands never reach a pane
    /// hidden behind another.
    ///
    /// Returns what is left for the window to do: panes the other window
    /// opened that this tab had no room left for, or that it emptied.
    #[must_use]
    fn refresh(&mut self, location: &Location) -> Refreshed {
        let Some(mut fresh) = Self::restore(self.host.clone(), location) else {
            return Refreshed::default();
        };
        let Some(Location::CoreTerminal {
            panes: base,
            name: base_name,
            names: base_names,
            ..
        }) = &self.saved
        else {
            *self = fresh;
            return Refreshed::default();
        };
        let mut left = Refreshed::default();
        if self.panes != *base {
            if fresh.panes == *base {
                fresh.panes = self.panes.clone();
            } else {
                let (panes, overflow) = merged(&self.panes, base, &fresh.panes, &self.active);
                left.overflow = overflow;
                match panes {
                    Some(panes) => fresh.panes = panes,
                    None => left.emptied = true,
                }
            }
        }
        // Names this window changed since it last read the tab stand too:
        if self.name != *base_name {
            fresh.name = self.name.take();
        }
        // Pane by pane, so a name given here and one given there both stand.
        let touched = (self.names.keys())
            .chain(base_names.keys())
            .filter(|terminal| self.names.get(*terminal) != base_names.get(*terminal));
        for terminal in touched.cloned().collect::<Vec<_>>() {
            match self.names.get(&terminal) {
                Some(name) => fresh.names.insert(terminal, name.clone()),
                None => fresh.names.remove(&terminal),
            };
        }
        fresh
            .names
            .retain(|terminal, _| fresh.panes.contains(terminal));
        if fresh.panes.contains(&self.active) {
            fresh.active = self.active.clone();
        } else if !fresh.panes.contains(&fresh.active) {
            // Both windows closed a pane, and the merge holds neither one
            // in use: the keyboard goes to a pane that is still there.
            fresh.active = fresh.panes.first().clone();
        }
        let zoomed = if self.unsaved {
            self.zoomed.clone().filter(|_| self.zoom.is_open())
        } else {
            fresh.zoomed.clone().filter(|_| fresh.zoom.is_open())
        }
        .filter(|zoomed| *zoomed == fresh.active && fresh.panes.count() > 1);
        let same = zoomed == self.zoomed.clone().filter(|_| self.zoom.is_open());
        fresh.zoom = match (&zoomed, same) {
            // Unchanged: an animation in flight carries on.
            (_, true) => self.zoom,
            (Some(_), false) => {
                let mut zoom = crate::motion::Toggle::default();
                zoom.set(true, crate::motion::now(), true);
                zoom
            }
            (None, false) => crate::motion::Toggle::default(),
        };
        fresh.zoomed = zoomed;
        fresh.root = self.root.take();
        fresh.workspace = self.workspace.take();
        fresh.previous = (self.previous.take()).filter(|previous| fresh.panes.contains(previous));
        fresh.unsaved = self.unsaved;
        fresh.saved = Some(location.clone());
        *self = fresh;
        left
    }

    /// The tab's location as it saves: the layout, the pane in use, and the
    /// zoomed pane while the zoom is on.
    fn location(&self) -> Location {
        Location::CoreTerminal {
            panes: self.panes.clone(),
            observe: self.observe,
            active: Some(self.active.clone()),
            zoomed: self.zoomed.clone().filter(|_| self.zoom.is_open()),
            name: self.name.clone(),
            names: self
                .names
                .iter()
                .filter(|(terminal, _)| self.panes.contains(terminal))
                .map(|(terminal, name)| (terminal.clone(), name.clone()))
                .collect(),
        }
    }

    /// Makes `terminal` the pane in use, remembering the one it replaces;
    /// whether that changed.
    fn activate(&mut self, terminal: TerminalId) -> bool {
        if self.active == terminal || !self.panes.contains(&terminal) {
            return false;
        }
        self.previous = Some(std::mem::replace(&mut self.active, terminal));
        self.unsaved = true;
        true
    }

    /// Zooms `terminal`, or unzooms, animating from where the zoom is now.
    fn set_zoom(&mut self, terminal: Option<TerminalId>) {
        let now = crate::motion::now();
        match terminal {
            Some(terminal) => {
                self.zoomed = Some(terminal);
                self.zoom.set(true, now, false);
            }
            None => self.zoom.set(false, now, false),
        }
    }

    /// The zoomed pane while it shows, and how much of the tab it fills.
    fn zoom_at(&self, now: std::time::Instant) -> Option<(&TerminalId, f32)> {
        let zoomed = self.zoomed.as_ref()?;
        (self.zoom.is_open() || self.zoom.moving(now)).then(|| (zoomed, self.zoom.shown(now)))
    }

    /// The panes drawn at `now`: a zoom that has landed hides the others.
    fn drawn_at(&self, now: std::time::Instant) -> Vec<&TerminalId> {
        match self.zoom_at(now) {
            Some((zoomed, shown)) if shown >= 1. => vec![zoomed],
            _ => self.panes.terminals(),
        }
    }
}

/// The most terminals waiting to start at once.
const MAX_WANTED: usize = 16;

/// The window's herdr-core state.
#[derive(Default)]
pub(crate) struct CoreTerminals {
    /// When terminals labelled for a workspace were last looked at.
    adopt_checked: Option<std::time::Instant>,
    /// Terminals labelled for a workspace that were put in a tab, so each
    /// is put in one only once.
    adopted: std::collections::HashSet<TerminalId>,
    /// The Git checkout of each kept workspace's folder and of the shown
    /// pane's.
    checkouts: checkouts::Checkouts,
    /// The views the window drew in its last frame: a cached view of a tab
    /// no group shows is not among them.
    drawn: Vec<EntityId>,
    /// The shell shown when no Herdr 1 is installed.
    fallback: fallback::Fallback,
    /// A shown tab whose active pane should take the keyboard once its view
    /// exists.
    focus_wanted: Option<TabId>,
    pub(crate) hub: Hub,
    infos: infos::Infos,
    /// How the views draw now, to tell when the window's look changed.
    look: Option<Look>,
    /// The kept workspace the user brought to the front, by its host.
    in_front: Option<(Scope, String)>,
    /// When each agent of `sessions` last changed state, to order it among
    /// Herdr 1's agents, per core: terminal IDs are unique only within one.
    pub(crate) recency: HashMap<Scope, crate::state::StatusRecency>,
    /// A kept workspace just came to the front: show one of its tabs.
    reveal: bool,
    /// The sessions over herdr-core, by the scope of their workspaces: this
    /// computer's, then each saved host's whose workspaces the app keeps.
    sessions: Vec<(Scope, std::sync::Arc<sheprd_session::Session>)>,
    /// A phone's split on its way, answered once it lands or fails.
    phone_split: Option<phone::Pending>,
    /// The tab the active group shows and its pane in use, as of the last
    /// tick, for work that runs without the app's state at hand.
    shown: Option<(TabId, TerminalId)>,
    /// Other hosts' cores, by the scope of their workspaces.
    remote: HashMap<Scope, hosts::Remote>,
    /// A shell starting, and what for.
    starting: Option<(Purpose, Host, Started)>,
    tabs: HashMap<TabId, TabPanes>,
    /// Terminals a kill did not reach, as when the connection dropped while
    /// it was sent, put back to end once their core answers again. Every
    /// window holds the app's one list ([`tabs::Unended`]).
    unended: std::sync::Arc<std::sync::Mutex<Vec<crate::browser::Ending>>>,
    views: HashMap<(TabId, TerminalId), Entity<CoreTerminalView>>,
    /// Terminals to start, one after another, once the hub of each one's
    /// host connects: a shell, or the program its spawn names. Several
    /// workspaces made in a row each wait their turn rather than replace one
    /// another.
    wanted: VecDeque<Wanted>,
}

impl CoreTerminals {
    /// Every view on screen: the tabs' panes and the stand-in shell.
    fn shown(&self) -> impl Iterator<Item = &Entity<CoreTerminalView>> {
        let fallback = match &self.fallback {
            fallback::Fallback::Shown(view) => Some(view),
            _ => None,
        };
        self.views.values().chain(fallback)
    }

    /// Whether any shown terminal has output the window has not drawn.
    pub(crate) fn updated(&self, cx: &App) -> bool {
        self.shown().any(|view| view.read(cx).changed())
    }

    /// Whether a herdr-core terminal holds the keyboard.
    pub(crate) fn focused(&self, window: &Window, cx: &App) -> bool {
        self.shown()
            .any(|view| view.read(cx).focus_handle().contains_focused(window, cx))
    }

    /// What this computer's core reports of the terminals in the panes of
    /// tabs that hold chats, which a chat may be drawn over. A mirror only
    /// watches, and a tab on another host runs no chat.
    pub(crate) fn chat_terminals(&self) -> impl Iterator<Item = &TerminalInfo> {
        self.tabs
            .values()
            .filter(|shown| shown.holds_chats())
            .flat_map(|shown| shown.panes.terminals())
            .filter_map(|terminal| self.infos.get(terminal))
    }

    /// What `host`'s core last reported of `terminal`, while it answers. A
    /// lost core's last list may name agents that have since stopped, so
    /// none is reported until it answers again.
    pub(crate) fn reported(&self, host: &Host, terminal: &TerminalId) -> Option<&TerminalInfo> {
        self.backend_of(host)?;
        self.infos_of(host)?.get(terminal)
    }

    /// The session over the core whose workspaces `scope` names, while the
    /// app keeps one.
    pub(crate) fn session_of(
        &self,
        scope: &Scope,
    ) -> Option<&std::sync::Arc<sheprd_session::Session>> {
        self.sessions
            .iter()
            .find(|(known, _)| known == scope)
            .map(|(_, session)| session)
    }

    /// The pane a new chat in `tab` opens beside: the one in use, in a tab
    /// that holds chats.
    pub(crate) fn chat_split_at(&self, tab: TabId) -> Option<TerminalId> {
        self.tabs
            .get(&tab)
            .filter(|shown| shown.holds_chats())
            .map(|shown| shown.active.clone())
    }

    /// Shows `panes` as `tab`, a mirror when `observe`, with what herdr-core
    /// reports of `infos`, as a restored tab and a finished list would.
    #[cfg(test)]
    pub(crate) fn show_for_test(
        &mut self,
        tab: TabId,
        panes: Panes,
        observe: bool,
        infos: Vec<TerminalInfo>,
    ) {
        let mut shown = TabPanes::new(Host::Local, panes, observe);
        for info in infos {
            self.infos.insert(info);
        }
        shown.keep_root(&self.infos);
        self.tabs.insert(tab, shown);
    }

    /// Takes `identity` as what Git answered for the folder `cwd`.
    #[cfg(test)]
    pub(crate) fn seed_checkout_for_test(
        &mut self,
        cwd: &str,
        identity: Option<sheprd_vcs::git::Identity>,
    ) {
        self.checkouts.seed(cwd, identity);
    }

    /// Places `tab` in `workspace` of `scope`, as showing the workspace does.
    #[cfg(test)]
    pub(crate) fn place_for_test(&mut self, tab: TabId, scope: Scope, workspace: &str) {
        if let Some(shown) = self.tabs.get_mut(&tab) {
            shown.workspace = Some((scope, workspace.to_owned()));
        }
    }

    /// Whether `tab` is a herdr-core tab that holds chats: it drives its
    /// terminals, rather than a mirror that only watches, on this computer.
    pub(crate) fn drives(&self, tab: TabId) -> bool {
        self.tabs.get(&tab).is_some_and(TabPanes::holds_chats)
    }

    /// The panes a tab that holds chats draws: all of them, or only the
    /// zoomed one once it covers the split. A mirror draws no chat.
    pub(crate) fn visible_chat_panes(&self, tab: TabId) -> Vec<&TerminalId> {
        let Some(shown) = self.tabs.get(&tab).filter(|shown| shown.holds_chats()) else {
            return Vec::new();
        };
        let zoomed = shown
            .zoom_at(crate::motion::now())
            .filter(|(_, amount)| *amount >= 1.)
            .map(|(terminal, _)| terminal);
        shown
            .panes
            .terminals()
            .into_iter()
            .filter(|terminal| zoomed.is_none_or(|zoomed| zoomed == *terminal))
            .collect()
    }

    /// Makes `terminal` the pane in use in `tab`; whether that changed.
    pub(crate) fn set_active(&mut self, tab: TabId, terminal: TerminalId) -> bool {
        self.tabs
            .get_mut(&tab)
            .is_some_and(|shown| shown.activate(terminal))
    }

    /// The tab whose panes hold `terminal` of this computer's core, among
    /// tabs that hold chats. Another host's `t1` is another terminal.
    pub(crate) fn tab_of(&self, terminal: &TerminalId) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|(_, shown)| shown.holds_chats() && shown.panes.contains(terminal))
            .map(|(id, _)| *id)
    }

    /// How the agents in `panes` on `host` are doing, as their tab's mark
    /// shows it: the status that most wants attention, or unknown with no
    /// agent. Terminal ids are only unique within a host.
    pub(crate) fn tab_status(&self, host: &Host, panes: &Panes) -> AgentStatus {
        let Some(infos) = self.infos_of(host) else {
            return AgentStatus::Unknown;
        };
        panes
            .terminals()
            .into_iter()
            .filter_map(|terminal| infos.get(terminal))
            .map(|info| info.agent)
            .max_by_key(|status| crate::sidebar::status_priority(*status))
            .unwrap_or(AgentStatus::Unknown)
    }

    /// Whether the user named `tab`.
    pub(crate) fn tab_named(&self, tab: TabId) -> bool {
        self.tabs
            .get(&tab)
            .is_some_and(|shown| shown.name.is_some())
    }

    /// Whether a tab on this computer's core shows `terminal`, a mirror
    /// included, which [`Self::tab_of`] leaves out.
    pub(crate) fn mirrors(&self, terminal: &TerminalId) -> bool {
        self.tabs
            .values()
            .any(|shown| shown.host == Host::Local && shown.panes.contains(terminal))
    }

    /// The tab and pane whose view holds the keyboard.
    pub(crate) fn focused_pane(&self, window: &Window, cx: &App) -> Option<(TabId, TerminalId)> {
        self.views
            .iter()
            .find(|(_, view)| view.read(cx).focus_handle().is_focused(window))
            .map(|(key, _)| key.clone())
    }
}

impl SheprdWindow {
    /// Whether a new tab runs in herdr-core: the core of the host the
    /// focused workspace is on. A host found to have no herdr-core keeps its
    /// new tabs in its Herdr.
    pub(crate) fn new_tabs_use_core(&self) -> bool {
        let host = Host::of(&crate::browser::scope(
            &self.endpoints[self.selected_endpoint],
        ));
        (self.config.herdr_2_terminals && !self.core_terminals.lacks_core(&host))
            || self.new_workspaces_use_core()
            || self.kept_in_front().is_some()
    }

    /// Whether a new workspace is one the app keeps for Herdr 2: with Herdr
    /// 2 chosen for new terminals, or with no Herdr 1 to make it. On a saved
    /// host that is one whose core answers while its Herdr 1 does not, and
    /// never one found to have no herdr-core.
    pub(crate) fn new_workspaces_use_core(&self) -> bool {
        if !self.selected_is_remote() {
            return self.config.herdr_2_terminals || self.live.missing_installation;
        }
        let host = Host::of(&crate::browser::scope(
            &self.endpoints[self.selected_endpoint],
        ));
        if self.core_terminals.lacks_core(&host) {
            return false;
        }
        let herdr_1_unavailable = self.live.herdr_1_unavailable()
            || (self.live.snapshot.is_none() && self.core_terminals.backend_of(&host).is_some());
        self.config.herdr_2_terminals || herdr_1_unavailable
    }

    /// The `new_core_terminal` command: starts a shell in herdr-core and
    /// shows it in a tab of the focused workspace. With no core running on
    /// this computer, the user's own `herdr-core` or the bundled one starts;
    /// on another host, its bridge starts the host's own.
    pub(crate) fn new_core_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((scope, workspace)) = self.browser_key() else {
            self.show_flash(Flash::warning(fl!("core-terminals-open-workspace")), cx);
            return;
        };
        self.dismiss_menu(window, cx);
        // A kept workspace's folder is the app's to give: herdr-core knows
        // no workspaces to start a shell in.
        let spawn = Spawn {
            cwd: self
                .kept_in_front()
                .and_then(|_| self.front_workspace_cwd()),
            ..Spawn::default()
        };
        self.start_core_program(Purpose::Tab { scope, workspace }, spawn, cx);
    }

    /// Starts `spawn` in herdr-core for `purpose`, once the hub of its
    /// host's core connects, in the folder and with the Herdr variables of
    /// where it opens.
    pub(crate) fn start_core_program(&mut self, purpose: Purpose, spawn: Spawn, cx: &App) {
        self.start_core_program_with(purpose, spawn, None, cx);
    }

    /// Starts `spawn` as [`Self::start_core_program`] does, handing `file`
    /// to its program once it starts. Until then the file is the window's,
    /// removed if the start fails or the request is dropped.
    pub(crate) fn start_core_program_with(
        &mut self,
        purpose: Purpose,
        mut spawn: Spawn,
        file: Option<tempfile::TempPath>,
        cx: &App,
    ) {
        if self.core_terminals.wanted.len() >= MAX_WANTED {
            tracing::warn!("Too many herdr-core terminals waiting to start; dropped one");
            return;
        }
        let host = self.purpose_host(&purpose);
        let Some(hub) = self.core_hub(&host) else {
            return;
        };
        hub.connect(Start::IfMissing);
        self.give_core_context(&purpose, &host, &mut spawn, cx);
        self.core_terminals
            .wanted
            .push_back((purpose, host, spawn, file));
        self.start_core_shell();
    }

    /// The core a terminal started for `purpose` runs in, read once as it is
    /// asked for: a tab closed while it starts must not send its terminal
    /// to another core.
    fn purpose_host(&self, purpose: &Purpose) -> Host {
        match purpose {
            Purpose::Tab { scope, .. } => Host::of(scope),
            Purpose::Split { tab, .. } => self.core_terminals.host_of(*tab),
        }
    }

    /// Starts the wanted shell once the hub is connected.
    fn start_core_shell(&mut self) {
        if self.core_terminals.starting.is_some() {
            return;
        }
        let terminals = &mut self.core_terminals;
        let Some((_, host, _, _)) = terminals.wanted.front() else {
            return;
        };
        let Some(backend) = terminals.backend_of(host).cloned() else {
            return;
        };
        let Some((purpose, host, spawn, file)) = terminals.wanted.pop_front() else {
            return;
        };
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("herdr-core-spawn".into())
            .spawn(move || {
                let started = backend.spawn(&spawn);
                // The program removes its file; a failed start leaves it
                // here, where dropping it removes it.
                if started.is_ok()
                    && let Some(file) = file
                    && let Err(error) = file.keep()
                {
                    tracing::warn!(%error, "Could not hand a file to a herdr-core program");
                }
                let _ = send.send(started);
            });
        match spawned {
            Ok(_) => terminals.starting = Some((purpose, host, receive)),
            Err(error) => tracing::warn!(%error, "Could not start a herdr-core spawn worker"),
        }
    }

    /// Runs every window tick: connects for restored tabs, places a started
    /// terminal, gives each shown pane its view, and takes new screens.
    /// Syncs the tabs' views, and keeps the keyboard in a tab whose pane
    /// that held it another window closed: the pane now in use there takes
    /// it. Only a pane that held it hands it on, so a control the user is
    /// in elsewhere keeps it.
    pub(super) fn sync_core_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.core_terminals.focused_pane(window, cx);
        self.sync_core_views(cx);
        let Some((tab, terminal)) = focused else {
            return;
        };
        let gone = (self.core_terminals.tabs.get(&tab))
            .is_some_and(|shown| !shown.panes.contains(&terminal));
        if !gone {
            return;
        }
        if self.core_pane_view(tab).is_some() {
            self.focus_core_terminal(tab, window, cx);
        } else {
            self.core_terminals.focus_wanted = Some(tab);
        }
    }

    pub(crate) fn poll_core_terminals(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.poll_core_hubs(window, cx);
        self.place_started_core_terminal(window, cx);
        self.end_closed_core_terminals(cx);
        self.relink_stale_core_views(cx);
        // The next terminal waiting starts once the last one answered.
        self.start_core_shell();
        self.sync_core_tabs(window, cx);
        if let Some(tab) = self.core_terminals.focus_wanted
            && self.core_pane_view(tab).is_some()
        {
            // Focus the user gave something else while the pane waited, such
            // as a picker's search field, is newer than the wish: keep it.
            if keyboard_free(window.focused(cx).as_ref(), &self.focus) {
                self.focus_core_terminal(tab, window, cx);
            } else {
                self.core_terminals.focus_wanted = None;
            }
        }
        self.poll_core_fallback(cx);
        self.refresh_core_looks(cx);
        // The stand-in shell rings only while its workspace is shown, as a
        // tab does; its clipboard writes land either way.
        let fallback_shown = self.default_workspace_shown() && !self.core_fallback_silenced(cx);
        self.poll_core_views(
            self.core_fallback_view()
                .map(|view| (view, fallback_shown.then_some(Host::Local))),
            window,
            cx,
        );
        self.poll_core_panes(window, cx);
        self.poll_core_edits(cx);
        // While a phone may start work here, this computer's core is joined
        // (never started) to see it, with or without a tab of its own.
        if crate::remote::supervisor::Supervisor::enabled(cx) {
            self.core_terminals.hub.connect(Start::Join);
        }
        // Split chrome and the sidebar's rows name panes from what herdr-core
        // reports, and the shell shown in place of Herdr 1 reads its links
        // against its folder, which this computer's core reports.
        let fallback = self.core_fallback_view();
        self.answer_phone_split(cx);
        let listed = self.core_terminals.poll_infos(std::time::Instant::now());
        self.adopt_labelled_terminals(listed, cx);
        if listed {
            self.hear_core_agents(cx);
            cx.notify();
        }
        if let Some(view) = fallback {
            let cwd = self
                .core_terminals
                .infos_of(&Host::Local)
                .and_then(|infos| infos.named(view.read(cx).terminal()))
                .and_then(|info| info.cwd.clone());
            view.update(cx, |view, cx| view.set_cwd(cwd.as_deref(), cx));
        }
        self.core_terminals.shown = self.shown_core_pane(cx);
        self.tell_core_agents(window, cx);
        if !cx.has_active_drag() {
            self.save_core_panes(cx);
        }
        if self.refresh_core_session(cx) {
            cx.notify();
        }
        self.reveal_kept_workspace(window, cx);
        // A zoom in motion draws a frame each tick until it lands.
        let now = crate::motion::now();
        if self
            .core_terminals
            .tabs
            .values()
            .any(|tab| tab.zoom.moving(now))
        {
            cx.notify();
        }
    }

    fn place_started_core_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, _, started)) = &self.core_terminals.starting else {
            return;
        };
        let outcome = match started.try_recv() {
            Ok(outcome) => outcome,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.core_terminals.starting = None;
                return;
            }
        };
        let Some((purpose, host, _)) = self.core_terminals.starting.take() else {
            return;
        };
        let info = match outcome {
            // Known at once, so a chat it labels attaches without waiting
            // for the next list.
            Ok(info) => match self.core_terminals.infos_of_mut(&host) {
                Some(infos) => infos.insert(info),
                None => info,
            },
            Err(error) => {
                let reason = error.to_string();
                self.show_flash(
                    Flash::warning(fl!("core-terminals-start-failed", reason = reason)),
                    cx,
                );
                return;
            }
        };
        match purpose {
            // The kept workspace it was for closed while it started: end it
            // rather than leave a shell no tab shows.
            Purpose::Tab { scope, workspace }
                if self.kept_workspace_gone(&scope, &workspace, cx) =>
            {
                self.end_core_terminal(&host, info.id);
            }
            Purpose::Tab { scope, workspace } => {
                let location = Location::core_terminal(Panes::single(info.id), false);
                let Some(id) = Store::update(cx, |store| {
                    store.open(scope, &workspace, Some(location), None)
                }) else {
                    self.show_flash(Flash::warning(fl!("core-terminals-too-many-tabs")), cx);
                    return;
                };
                self.sync_core_views(cx);
                self.show_browser_tab(id, window, cx);
            }
            Purpose::Split { tab, at, axis } => {
                self.place_split(tab, &at, axis, (host, info.id), window, cx)
            }
        }
        cx.notify();
    }

    /// Gives a herdr-core tab's active pane the keyboard as the tab is
    /// shown, or as soon as its view exists for a restored tab still waiting
    /// on the hub.
    pub(crate) fn focus_core_terminal(&mut self, tab: TabId, window: &mut Window, cx: &mut App) {
        self.core_terminals.focus_wanted = None;
        // A pane drawn as a chat gives its chat the keyboard.
        let chat = self
            .core_terminals
            .tabs
            .get(&tab)
            .and_then(|shown| self.core_chat_focus(tab, &shown.active, window, cx));
        if let Some(focus) = chat {
            window.focus(&focus, cx);
            return;
        }
        match self.core_pane_view(tab) {
            Some(view) => window.focus(&view.read(cx).focus_handle().clone(), cx),
            None => self.core_terminals.focus_wanted = Some(tab),
        }
    }

    /// The view of `tab`'s active pane.
    fn core_pane_view(&self, tab: TabId) -> Option<&Entity<CoreTerminalView>> {
        let active = &self.core_terminals.tabs.get(&tab)?.active;
        self.core_terminals.views.get(&(tab, active.clone()))
    }

    /// How the window draws terminals, for its herdr-core views.
    pub(crate) fn core_look(&self) -> Look {
        let shared = self.settings.shared.as_ref();
        Look {
            background: self.fill().base(self.terminal_theme().background),
            cell_height: self.config.terminal.line_height(),
            cell_width: self.cell_width,
            chrome: std::rc::Rc::new(view::Chrome {
                theme: self.theme.clone(),
                ui: self.config.ui.clone(),
            }),
            copy_on_select: shared.is_none_or(|shared| shared.copy_on_select),
            font: self.config.terminal.font(),
            ground: self.ground(),
            keep_selection: self.config.keep_selection_after_copy,
            option_as_alt: self.config.option_as_alt,
            padding: self.config.terminal_padding,
            right_click_passthrough: shared
                .map(|shared| shared.right_click_passthrough)
                .unwrap_or_default(),
            scroll_lines: shared.map_or(
                crate::herdr_settings::DEFAULT_MOUSE_SCROLL_LINES,
                |shared| shared.mouse_scroll_lines,
            ),
            smooth_scroll: self.config.smooth_scroll,
        }
    }

    /// Keeps every view drawing as the window does after a font, theme, or
    /// option change.
    fn refresh_core_looks(&mut self, cx: &mut Context<Self>) {
        if self.core_terminals.shown().next().is_none() {
            return;
        }
        let look = self.core_look();
        if self.core_terminals.look.as_ref() == Some(&look) {
            return;
        }
        let views: Vec<Entity<CoreTerminalView>> = self.core_terminals.shown().cloned().collect();
        for view in views {
            view.update(cx, |view, cx| view.set_look(look.clone(), cx));
        }
        self.core_terminals.look = Some(look);
    }
}

/// Whether a pane waiting for the keyboard may take it: nothing holds it, or
/// only the window itself does.
fn keyboard_free(focused: Option<&FocusHandle>, window: &FocusHandle) -> bool {
    focused.is_none_or(|focused| focused == window)
}
