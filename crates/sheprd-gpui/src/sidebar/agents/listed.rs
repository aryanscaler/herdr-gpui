//! An agent as the sidebar lists it, whichever Herdr runs it: a Herdr 1
//! agent from its host's snapshot, or a Herdr 2 agent from this computer's
//! herdr-core session, which the app keeps. Both kinds share the agents
//! list, its order, its rows, and the previous, next, and numbered steps.

use super::status_priority;
use crate::{preferences::AgentSort, state::LiveState};
use sheprd_backend::{AgentStatus, TerminalInfo};
use sheprd_client::protocol::{ClientShellAgent, ClientShellSnapshot};
use sheprd_session::{Agent, Session};

/// One agent row, of either Herdr.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Listed<'a> {
    Herdr1 {
        agent: &'a ClientShellAgent,
        snapshot: &'a ClientShellSnapshot,
    },
    Core(CoreAgent<'a>),
}

/// A Herdr 2 agent: its place in the session, and what the core last said
/// of its terminal.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CoreAgent<'a> {
    pub(crate) agent: &'a Agent,
    pub(crate) session: &'a Session,
    /// The host's Herdr 1 snapshot, whose workspaces a Herdr 2 tab opened
    /// in one of them is placed in.
    pub(crate) snapshot: Option<&'a ClientShellSnapshot>,
    pub(crate) info: Option<&'a TerminalInfo>,
    /// Whether its pane is the one the window shows in use.
    pub(crate) focused: bool,
}

impl<'a> Listed<'a> {
    /// What a step to it and the window's own focus compare: the Herdr 1
    /// pane, or the herdr-core terminal.
    pub(crate) fn id(&self) -> &'a str {
        match self {
            Self::Herdr1 { agent, .. } => &agent.pane_id,
            Self::Core(core) => &core.agent.pane,
        }
    }

    /// The workspace it runs in, by id.
    pub(crate) fn workspace(&self) -> &'a str {
        match self {
            Self::Herdr1 { agent, .. } => &agent.workspace_id,
            Self::Core(core) => &core.agent.workspace,
        }
    }

    pub(crate) fn status(&self) -> AgentStatus {
        match self {
            Self::Herdr1 { agent, .. } => agent.agent_status,
            Self::Core(core) => core.agent.status,
        }
    }

    pub(crate) fn is_core(&self) -> bool {
        matches!(self, Self::Core(_))
    }

    pub(crate) fn focused(&self) -> bool {
        match self {
            Self::Herdr1 { agent, .. } => agent.focused,
            Self::Core(core) => core.focused,
        }
    }

    /// What it is called wherever it is listed.
    pub(crate) fn name(&self) -> &'a str {
        match self {
            Self::Herdr1 { agent, .. } => super::agent_name(agent),
            Self::Core(core) => &core.agent.name,
        }
    }

    /// Which agent it is, such as `claude`.
    pub(crate) fn identity(&self) -> Option<&'a str> {
        match self {
            Self::Herdr1 { agent, .. } => agent.agent.as_deref(),
            Self::Core(core) => core.agent.identity.as_deref(),
        }
    }

    /// The mark its row leads with.
    pub(crate) fn icon(&self, style: &crate::config::SidebarStyle) -> crate::icons::AgentIcon {
        match self {
            Self::Herdr1 { agent, snapshot } => super::agent_icon(agent, snapshot, style),
            Self::Core(core) => {
                crate::icons::AgentIcon::from_identity(core.agent.identity.as_deref())
            }
        }
    }

    /// Its workspace, and its tab when that earns a place.
    pub(crate) fn place(&self) -> Option<(&'a str, Option<&'a str>)> {
        match self {
            Self::Herdr1 { agent, snapshot } => super::agent_place(agent, snapshot),
            Self::Core(core) => core.place(),
        }
    }

    /// Its pane's name, never the agent's name again.
    pub(crate) fn pane(&self) -> Option<String> {
        match self {
            Self::Herdr1 { agent, snapshot } => super::pane_name(agent, snapshot, self.name()),
            Self::Core(core) => core.pane(),
        }
    }

    /// The words for its status: a Herdr 1 integration's own label when it
    /// set one, else `fallback`.
    pub(crate) fn state_label<'f>(&self, fallback: &'f str) -> std::borrow::Cow<'f, str> {
        match self {
            Self::Herdr1 { agent, .. } => super::state_label(agent, fallback),
            Self::Core(_) => std::borrow::Cow::Borrowed(fallback),
        }
    }

    /// Where a click on its row goes.
    pub(crate) fn target(&self) -> Target {
        match self {
            Self::Herdr1 { agent, .. } => Target::Pane(agent.pane_id.clone()),
            Self::Core(core) => Target::Core {
                workspace: core.agent.workspace.clone(),
                tab: core.agent.tab.clone(),
                terminal: core.agent.pane.clone(),
            },
        }
    }
}

/// Where a listed agent is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// A Herdr 1 pane of the agent's host.
    Pane(String),
    /// A herdr-core terminal in a tab of a workspace, as a `Session` names
    /// them.
    Core {
        workspace: String,
        tab: String,
        terminal: String,
    },
}

impl crate::SheprdWindow {
    /// Shows the agent `target` names, on the host `endpoint`, with the
    /// keyboard in its pane.
    pub(crate) fn open_listed_agent(
        &mut self,
        endpoint: &str,
        target: &Target,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        match target {
            Target::Pane(pane) => {
                self.navigate_endpoint(endpoint, crate::NavigationTarget::Pane(pane), cx);
                window.focus(&self.focus, cx);
            }
            Target::Core {
                workspace,
                tab,
                terminal,
            } => self.show_core_agent(endpoint, workspace, tab, terminal, window, cx),
        }
    }
}

impl<'a> CoreAgent<'a> {
    /// Where it runs, as a Herdr 1 agent's place reads: its workspace, and
    /// its tab when the workspace has several or the user named it. `None`
    /// for the shell shown in place of Herdr 1, whose workspace the app
    /// keeps for it alone.
    pub(crate) fn place(&self) -> Option<(&'a str, Option<&'a str>)> {
        let Some(workspace) = self.session.workspace(&self.agent.workspace) else {
            let workspace = self
                .snapshot?
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == self.agent.workspace)?;
            return Some((workspace.label.as_str(), None));
        };
        let tab = workspace
            .tabs
            .iter()
            .find(|tab| tab.id == self.agent.tab)
            .filter(|_| workspace.tabs.len() > 1)
            .and_then(|tab| sheprd_client::text::caption(&tab.label));
        Some((workspace.label.as_str(), tab))
    }

    /// The values plugins and hooks reported for it, as labels on its
    /// terminal (`hc label`), by name: Herdr 2's counterpart of the custom
    /// `$name` values Herdr 1 agents report.
    pub(crate) fn plugin_values(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.info
            .into_iter()
            .flat_map(|info| &info.labels)
            .filter(|(name, _)| !own_label(name))
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    /// The terminal's own title, when it says more than the agent's name:
    /// untrusted text, made safe and bounded.
    pub(crate) fn pane(&self) -> Option<String> {
        let title = self.info?.title.as_deref()?;
        let title = sheprd_client::text::caption(title)?;
        let title = crate::notifications::safe_text(title, super::PANE_CAPTION_LIMIT)
            .trim()
            .to_owned();
        (!title.is_empty() && title != self.agent.name.trim()).then_some(title)
    }
}

/// Whether a herdr-core label is one Sheprd itself sets, such as a chat's
/// endpoint or the workspace a terminal was started for, rather than a
/// value a plugin or hook reported.
pub(crate) fn own_label(name: &str) -> bool {
    name == sheprd_chat_protocol::TOKEN || name.starts_with("sheprd.")
}

/// The Herdr 2 agents of this computer's session, listed with the host at
/// `index`, and when each last changed state.
pub(crate) struct CoreAgents<'a> {
    pub(crate) index: usize,
    /// Whether the core answers, which a connected Herdr 1 host compares to.
    pub(crate) answers: bool,
    pub(crate) agents: Vec<CoreAgent<'a>>,
    /// None before the core's agents were first observed.
    pub(crate) recency: Option<&'a crate::state::StatusRecency>,
}

/// Every listed host's agents in the order the panel paints them, as
/// [`super::panel_agents`] orders Herdr 1's, with each host's Herdr 2 agents,
/// `cores`, among them: grouped, after their host's own; by priority,
/// merged with every host's by attention and then the latest change, a core
/// counted as connected while it answers. `hosts` are in sidebar order; a
/// core whose host is not among them lists nothing.
pub(crate) fn listed_agents<'a>(
    hosts: Vec<(usize, &'a LiveState)>,
    cores: impl IntoIterator<Item = CoreAgents<'a>>,
    sort: AgentSort,
) -> Vec<(usize, Listed<'a>)> {
    let lives = hosts.clone();
    let herdr_1 = super::panel_agents(hosts, sort);
    let order = |index: usize| lives.iter().position(|(host, _)| *host == index);
    let mut cores: Vec<CoreAgents<'a>> = cores
        .into_iter()
        .filter(|core| !core.agents.is_empty() && order(core.index).is_some())
        .collect();
    let mut rows: Vec<(usize, Listed<'a>)> = herdr_1
        .into_iter()
        .filter_map(|(index, agent)| herdr_1_row(&lives, index, agent))
        .collect();
    if cores.is_empty() {
        return rows;
    }
    // Grouped lists one host after another: a core's agents go after the
    // last of its host's own, before the next host's. The later hosts go
    // in first, so an earlier one's place does not move.
    cores.sort_by_key(|core| std::cmp::Reverse(order(core.index)));
    for core in &cores {
        let split = rows
            .iter()
            .position(|(index, _)| order(*index) > order(core.index))
            .unwrap_or(rows.len());
        let core_rows = core
            .agents
            .iter()
            .map(|agent| (core.index, Listed::Core(*agent)));
        rows.splice(split..split, core_rows);
    }
    // A plugin view orders its own host's agents, so then each keeps its
    // order, as Herdr 1's hosts do.
    let viewed = lives.iter().any(|(_, live)| {
        live.snapshot
            .as_deref()
            .is_some_and(|snapshot| snapshot.agent_view_label.is_some())
    });
    if sort == AgentSort::Priority && !viewed {
        let live = |index: usize| {
            lives
                .iter()
                .find(|(host, _)| *host == index)
                .map(|(_, live)| *live)
        };
        let core_of = |index: usize| cores.iter().find(|core| core.index == index);
        rows.sort_by_key(|(index, listed)| {
            let (connected, recency) = match listed {
                Listed::Herdr1 { agent, .. } => live(*index).map_or((false, 0), |live| {
                    (
                        live.status.is_connected(),
                        live.agent_recency.of(&agent.pane_id),
                    )
                }),
                Listed::Core(agent) => core_of(*index).map_or((false, 0), |core| {
                    (
                        core.answers,
                        core.recency
                            .map_or(0, |recency| recency.of(&agent.agent.pane)),
                    )
                }),
            };
            (
                !connected,
                std::cmp::Reverse(status_priority(listed.status())),
                std::cmp::Reverse(recency),
            )
        });
    }
    rows
}

fn herdr_1_row<'a>(
    lives: &[(usize, &'a LiveState)],
    index: usize,
    agent: &'a ClientShellAgent,
) -> Option<(usize, Listed<'a>)> {
    let (_, live) = lives.iter().find(|(host, _)| *host == index)?;
    let snapshot = live.snapshot.as_deref()?;
    Some((index, Listed::Herdr1 { agent, snapshot }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
