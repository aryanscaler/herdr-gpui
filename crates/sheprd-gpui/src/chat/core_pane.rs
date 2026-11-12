//! Chats in herdr-core (Herdr 2) panes. herdr-core keeps no pane metadata
//! a program could report its endpoint through, so the window chooses the
//! endpoint itself when it starts `herdr-acp` in a terminal: it passes the
//! id in [`ENDPOINT_ENV`] and labels the terminal with it under [`TOKEN`].
//! herdr-core keeps the label, so this window finds the chat again after a
//! restart, as any other window does.
//!
//! The label is daemon data like Herdr's token. Local endpoints are derived
//! in this user's directory; remote endpoints are opened by ACP on their
//! owning host, through that saved host's authenticated transport.

use super::view::Place;
use super::{MAX_CHATS, Route, Surface, Target};
use crate::{
    SheprdWindow,
    browser::TabId,
    core_terminals::{CoreTerminalView, Purpose},
};
use sheprd_backend::{Spawn, TerminalId, TerminalInfo};
use sheprd_chat_protocol::{Agent, ENDPOINT_ENV, EndpointId, TOKEN};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

/// The variable naming the Herdr pane `herdr-acp` reports to.
const PANE_ENV: &str = "HERDR_PANE_ID";

/// The window's key for the chat in `terminal`. Herdr pane ids have no
/// `core:` workspace, and a clash is dropped rather than shared.
pub(crate) fn key(terminal: &TerminalId) -> String {
    key_on(&crate::core_terminals::Host::Local, terminal)
}

/// A core terminal's chat identity, scoped to its owning host. The length
/// prefix keeps colons in saved host scopes or terminal IDs unambiguous.
pub(crate) fn key_on(host: &crate::core_terminals::Host, terminal: &TerminalId) -> String {
    match host {
        crate::core_terminals::Host::Local => format!("core:{terminal}"),
        crate::core_terminals::Host::Remote(scope) => format!(
            "core@{}:{}:{terminal}",
            scope.as_str().len(),
            scope.as_str()
        ),
    }
}

/// The herdr-core terminal a [`key`] names, if it is one.
pub(crate) fn terminal_of(key: &str) -> Option<TerminalId> {
    TerminalId::new(key.strip_prefix("core:")?).ok()
}

/// The chats among `terminals`, in their order, one per terminal and at
/// most [`MAX_CHATS`]. A terminal on another machine this core added
/// (`box/t7`) runs its program there, out of reach.
#[cfg(test)]
pub(super) fn targets<'a>(terminals: impl IntoIterator<Item = &'a TerminalInfo>) -> Vec<Target> {
    targets_on(
        &crate::core_terminals::Host::Local,
        &Route::Local,
        terminals,
    )
}

#[cfg(test)]
fn targets_on<'a>(
    host: &crate::core_terminals::Host,
    route: &Route,
    terminals: impl IntoIterator<Item = &'a TerminalInfo>,
) -> Vec<Target> {
    bounded_targets(
        terminals
            .into_iter()
            .filter_map(|info| target_on(host, route, info)),
    )
}

fn bounded_targets(targets: impl IntoIterator<Item = Target>) -> Vec<Target> {
    let mut seen = HashSet::new();
    targets
        .into_iter()
        .filter(|target| seen.insert(target.pane_id.clone()))
        .take(MAX_CHATS)
        .collect()
}

fn target_on(
    host: &crate::core_terminals::Host,
    route: &Route,
    info: &TerminalInfo,
) -> Option<Target> {
    if info.id.as_str().contains('/') || info.state != sheprd_backend::RunState::Running {
        return None;
    }
    Some(Target {
        endpoint: sheprd_core_client::convert::chat_endpoint(info)?,
        existing: None,
        pane_id: key_on(host, &info.id),
        route: route.clone(),
        surface: match host {
            crate::core_terminals::Host::Local => Surface::Core(info.id.clone()),
            _ => Surface::RemoteCore {
                host: host.clone(),
                terminal: info.id.clone(),
            },
        },
    })
}

/// `herdr` followed by as many of `core` as fit under [`MAX_CHATS`], each
/// whose key no Herdr chat already holds.
pub(super) fn with_core(mut herdr: Vec<Target>, core: Vec<Target>) -> Vec<Target> {
    for target in core {
        if herdr.len() >= MAX_CHATS {
            break;
        }
        if !herdr.iter().any(|known| known.pane_id == target.pane_id) {
            herdr.push(target);
        }
    }
    herdr
}

/// The terminal that runs `program` as a chat with `agent`, listening on
/// `endpoint` and labelled with it, with the `[chat.env]` variables `extra`.
pub(super) fn spawn(
    agent: Agent,
    program: &Path,
    endpoint: &EndpointId,
    extra: &BTreeMap<String, String>,
) -> Spawn {
    let mut env: Vec<(String, String)> = extra
        .iter()
        .filter(|(name, _)| name.as_str() != ENDPOINT_ENV && name.as_str() != PANE_ENV)
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    env.push((ENDPOINT_ENV.to_owned(), endpoint.to_string()));
    // A core started from a Herdr pane passes that pane on; the chat must
    // not report itself there.
    env.push((PANE_ENV.to_owned(), String::new()));
    Spawn {
        argv: vec![program.to_string_lossy().into_owned(), agent.to_string()],
        env,
        labels: BTreeMap::from([(TOKEN.to_owned(), endpoint.to_string())]),
        // herdr-core names are unique handles, so two chats with one agent
        // could not share one; the program titles its pane instead.
        ..Spawn::default()
    }
}

fn remote_spawn(agent: Agent, endpoint: &EndpointId, extra: &BTreeMap<String, String>) -> Spawn {
    let mut request = spawn(agent, Path::new("sheprd-acp"), endpoint, extra);
    request.argv = vec![
        "/bin/sh".into(),
        "-c".into(),
        super::remote_program_script(&agent.to_string()),
    ];
    request
}

impl SheprdWindow {
    /// The chats in the panes of the focused workspace's herdr-core tabs.
    pub(super) fn core_chat_targets(&self) -> Vec<Target> {
        bounded_targets(
            self.core_terminals
                .routed_chat_terminals()
                .filter_map(|(host, info)| {
                    if let crate::core_terminals::Host::Remote(scope) = host
                        && self.core_terminals.remote_target(scope) != self.saved_target(scope)
                    {
                        return None;
                    }
                    let route = self.core_chat_route(host)?;
                    target_on(host, &route, info)
                }),
        )
    }

    pub(super) fn core_chat_route(&self, host: &crate::core_terminals::Host) -> Option<Route> {
        match host {
            crate::core_terminals::Host::Local => Some(Route::Local),
            crate::core_terminals::Host::Remote(scope) => {
                super::discovery::remote_route(self.saved_target(scope)?)
            }
        }
    }

    pub(crate) fn core_chat_key(&self, tab: TabId, terminal: &TerminalId) -> String {
        key_on(&self.core_terminals.host_of(tab), terminal)
    }

    /// Splits `tab`'s pane in use with a chat running `agent`, beside it in
    /// its folder.
    pub(crate) fn start_core_chat(
        &mut self,
        agent: Agent,
        tab: TabId,
        cx: &gpui::App,
    ) -> Result<(), super::StartError> {
        let at = self
            .core_terminals
            .chat_split_at(tab)
            .ok_or(super::StartError::NoWorkspace)?;
        self.start_core_chat_at(agent, tab, at, cx)
    }

    /// Explicit CLI pane keys resolve against the owning core, including
    /// ordinary terminal panes; an unknown key never falls through to Herdr 1.
    pub(crate) fn start_core_chat_beside(
        &mut self,
        agent: Agent,
        key: &str,
        cx: &gpui::App,
    ) -> Result<(), super::StartError> {
        let (host, info) = self
            .core_terminals
            .routed_chat_terminals()
            .find(|(host, info)| key_on(host, &info.id) == key)
            .ok_or(super::StartError::NoWorkspace)?;
        let tab = self
            .core_terminals
            .chat_tab_on(host, &info.id)
            .ok_or(super::StartError::NoWorkspace)?;
        let at = info.id.clone();
        self.start_core_chat_at(agent, tab, at, cx)
    }

    fn start_core_chat_at(
        &mut self,
        agent: Agent,
        tab: TabId,
        at: TerminalId,
        cx: &gpui::App,
    ) -> Result<(), super::StartError> {
        if at.as_str().contains('/') {
            return Err(super::StartError::NotLocal);
        }
        let spawn = self.prepare_core_chat(agent, &self.core_terminals.host_of(tab))?;
        self.chats.last_started = Some(agent);
        self.start_core_program(
            Purpose::Split {
                tab,
                at,
                axis: crate::core_terminals::panes::Axis::Row,
            },
            spawn,
            cx,
        );
        Ok(())
    }

    /// A kept workspace with no driven tab still starts its chat on its own
    /// core, rather than asking a missing or unrelated Herdr 1 for a tab.
    pub(crate) fn start_kept_core_chat(
        &mut self,
        agent: Agent,
        cx: &gpui::App,
    ) -> Result<(), super::StartError> {
        self.kept_in_front().ok_or(super::StartError::NoWorkspace)?;
        let (scope, workspace) = self.browser_key().ok_or(super::StartError::NoWorkspace)?;
        let mut spawn = self.prepare_core_chat(agent, &crate::core_terminals::Host::of(&scope))?;
        spawn.cwd = self.front_workspace_cwd();
        self.chats.last_started = Some(agent);
        self.start_core_program(Purpose::Tab { scope, workspace }, spawn, cx);
        Ok(())
    }

    fn prepare_core_chat(
        &self,
        agent: Agent,
        host: &crate::core_terminals::Host,
    ) -> Result<Spawn, super::StartError> {
        if !self.config.features.acp_chats {
            return Err(super::StartError::Disabled);
        }
        if let crate::core_terminals::Host::Remote(scope) = host
            && self
                .core_terminals
                .remote_target(scope)
                .is_some_and(|target| Some(target) != self.saved_target(scope))
        {
            return Err(super::StartError::NotConnected);
        }
        if self.core_chat_agent_missing(host, agent) {
            return Err(super::StartError::AgentMissing(agent));
        }
        let route = self
            .core_chat_route(host)
            .ok_or(super::StartError::NotLocal)?;
        let program = if route == Route::Local {
            Some(self.chats.program.found()?.to_owned())
        } else {
            None
        };
        let endpoint = EndpointId::random();
        let spawn = match program.as_deref() {
            Some(program) => spawn(agent, program, &endpoint, &self.config.chat.env),
            None => remote_spawn(agent, &endpoint, &self.config.chat.env),
        };
        Ok(spawn)
    }

    /// `view`, opening its pane's menu on a right-click the program does not
    /// take, as a Herdr pane does.
    fn core_pane_menu_area(
        &self,
        tab: TabId,
        terminal: &TerminalId,
        view: gpui::Entity<CoreTerminalView>,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        use gpui::{InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _};
        let terminal = terminal.clone();
        gpui::div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .size_full()
            .child(view)
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let target = crate::core_terminals::menu::Target::Pane {
                        tab,
                        terminal: terminal.clone(),
                    };
                    this.open_core_menu(target, event.position, window, cx);
                    this.menu.opening_right_click =
                        this.menu.page == Some(crate::menu::Page::CoreMenu);
                }),
            )
            .into_any_element()
    }

    /// `terminal`'s pane in `tab`: its view, with its chat drawn over it when
    /// one is attached, or the button that brings the chat back over a pane
    /// turned to its terminal. A mirror only watches, so it keeps the view.
    pub(crate) fn core_pane_body(
        &self,
        tab: TabId,
        terminal: &TerminalId,
        view: &gpui::Entity<CoreTerminalView>,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        use gpui::{IntoElement as _, ParentElement as _, Styled as _, div};
        let pane_id = self.core_chat_key(tab, terminal);
        let chat = self
            .core_terminals
            .drives(tab)
            .then(|| self.chats.pane(&pane_id))
            .flatten()
            .filter(|chat| chat.available());
        let Some(chat) = chat else {
            return self.core_pane_menu_area(tab, terminal, view.clone(), cx);
        };
        // The cells' last size shapes the chat's rail; it covers the pane.
        let size = view
            .read(cx)
            .bounds()
            .map(|bounds| bounds.size)
            .unwrap_or_default();
        let overlay = if self.chats.raw(&pane_id) {
            self.render_chat_switch(&pane_id, Place::Fill(size), cx)
        } else {
            self.render_chat(chat, Place::Fill(size), cx)
        };
        div()
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .size_full()
            // Turned to its terminal, the pane still opens its menu there.
            .child(self.core_pane_menu_area(tab, terminal, view.clone(), cx))
            .child(overlay)
            .into_any_element()
    }

    /// The herdr-core tab whose pane the chat keyed `pane_id` is drawn
    /// over, when it is a herdr-core pane's.
    pub(crate) fn core_chat_tab(&self, pane_id: &str) -> Option<TabId> {
        match &self.chats.pane(pane_id)?.target.surface {
            Surface::Core(terminal) => self.core_terminals.tab_of(terminal),
            Surface::RemoteCore { host, terminal } => {
                self.core_terminals.chat_tab_on(host, terminal)
            }
            Surface::Herdr => None,
        }
    }

    /// What takes the keyboard in `terminal`'s pane of `tab` when its chat
    /// shows: an open viewer, the conversation being read, or the composer.
    /// None in a mirror, which shows no chat.
    pub(crate) fn core_chat_focus(
        &self,
        tab: TabId,
        terminal: &TerminalId,
        window: &gpui::Window,
        cx: &gpui::App,
    ) -> Option<gpui::FocusHandle> {
        let pane_id = self.core_chat_key(tab, terminal);
        if !self.core_terminals.drives(tab) || self.chats.raw(&pane_id) {
            return None;
        }
        let pane = self.chats.pane(&pane_id).filter(|pane| pane.available())?;
        if let Some(focus) = pane.viewer.as_ref().and_then(|viewer| viewer.open_focus()) {
            return Some(focus.clone());
        }
        if pane.reading(window) {
            return pane.text_focus.clone();
        }
        pane.input_focus(cx)
    }

    /// Moves the keyboard from a herdr-core pane's terminal view to the chat
    /// drawn over it, as when the chat attaches while the pane has focus.
    pub(super) fn follow_core_chat_focus(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some((tab, terminal)) = self.core_terminals.focused_pane(window, cx) else {
            return;
        };
        if let Some(focus) = self.core_chat_focus(tab, &terminal, window, cx) {
            window.focus(&focus, cx);
        }
    }

    /// The key of the chat in the herdr-core pane that holds the keyboard,
    /// through its chat or its terminal view, in a tab that drives it.
    pub(crate) fn focused_core_chat_key(
        &self,
        window: &gpui::Window,
        cx: &gpui::App,
    ) -> Option<String> {
        let (tab, terminal) = self
            .focused_core_chat(window, cx)
            .or_else(|| self.core_terminals.focused_pane(window, cx))?;
        let pane_id = self.core_chat_key(tab, &terminal);
        (self.core_terminals.drives(tab) && self.chats.pane(&pane_id).is_some()).then_some(pane_id)
    }

    /// The herdr-core tab and pane whose chat holds the keyboard.
    pub(crate) fn focused_core_chat(
        &self,
        window: &gpui::Window,
        cx: &gpui::App,
    ) -> Option<(TabId, TerminalId)> {
        self.chats.panes().find_map(|pane| {
            let (tab, terminal) = match &pane.target.surface {
                Surface::Core(terminal) => (self.core_terminals.tab_of(terminal)?, terminal),
                Surface::RemoteCore { host, terminal } => {
                    (self.core_terminals.chat_tab_on(host, terminal)?, terminal)
                }
                Surface::Herdr => return None,
            };
            pane.focus_handles(cx)
                .any(|focus| focus.is_focused(window))
                .then(|| (tab, terminal.clone()))
        })
    }

    /// Makes the pane under the chat keyed `pane_id` the one in use, as a
    /// click on its terminal does: Herdr's focused pane once its frame is
    /// ready for input, or the herdr-core tab's active one, which needs no
    /// Herdr connection.
    pub(super) fn focus_chat_surface(&mut self, pane_id: &str, cx: &mut gpui::Context<Self>) {
        let surface = self
            .chats
            .pane(pane_id)
            .map(|pane| pane.target.surface.clone());
        match surface {
            Some(Surface::RemoteCore { host, terminal }) => {
                if let Some(tab) = self.core_terminals.chat_tab_on(&host, &terminal)
                    && self.core_terminals.set_active(tab, terminal)
                {
                    cx.notify();
                }
            }
            Some(Surface::Core(terminal)) => {
                if let Some(tab) = self.core_terminals.tab_of(&terminal)
                    && self.core_terminals.set_active(tab, terminal)
                {
                    cx.notify();
                }
            }
            Some(Surface::Herdr) if self.input_ready() => self.focus_clicked_pane(pane_id, cx),
            Some(Surface::Herdr) => {}
            None => {}
        }
    }

    /// The herdr-core tab `group` shows, else the active group, that a new
    /// chat splits rather than opening a Herdr tab.
    pub(crate) fn core_chat_group_tab(
        &self,
        group: Option<crate::browser::GroupId>,
        cx: &gpui::App,
    ) -> Option<TabId> {
        let group = group.or_else(|| self.active_group())?;
        match self.group_shown(group, cx) {
            crate::browser::Shown::Page(tab) => self.core_terminals.chat_split_at(tab).map(|_| tab),
            _ => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
