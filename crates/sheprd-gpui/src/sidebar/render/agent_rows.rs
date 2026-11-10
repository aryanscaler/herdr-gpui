//! The agents list: every listed host's agents in the panel's one order, so a
//! blocked agent on one host can sit above an idle one on another.

use crate::i18n::fl;
use crate::{
    SheprdWindow,
    sidebar::{
        Indicators, Listed,
        agents::status_text,
        cell::{AgentRow, Cell, RowContext, RowData, layout_for},
        layout::SidebarLook,
        line_height, tokens, wash,
    },
};
use gpui::{prelude::*, *};

impl SheprdWindow {
    /// Appends a row for each agent `panel_agents` lists, in that order, and
    /// returns the list with how many rows it painted and where the selected
    /// host's focused agent sits among them, for the reveal. A row's host
    /// shapes its own context, as the agent may belong to any listed host.
    pub(super) fn append_agent_rows(
        &self,
        mut list: Stateful<Div>,
        indicators: Indicators,
        look: SidebarLook,
        width: f32,
        cx: &mut Context<Self>,
    ) -> (Stateful<Div>, usize, Option<usize>) {
        let font = &self.config.sidebar;
        let theme = &self.theme;
        let multi = self.endpoints.len() > 1;
        let mut count = 0;
        let mut focused = None;
        for (index, agent) in self.panel_agents() {
            let Some(endpoint) = self.endpoints.get(index) else {
                continue;
            };
            let selected = index == self.selected_endpoint;
            let row_cx = RowContext {
                indicators,
                font,
                theme,
                look,
                width,
                // Agents list under their own heading, not under a host, but
                // each carries its host's colour.
                nest: 0.,
                mark: wash::HostMark::resolve(
                    &self.config.sidebar_style,
                    &endpoint.label,
                    selected,
                    theme,
                ),
                host: (multi && endpoint.id != crate::endpoint::LOCAL)
                    .then_some(endpoint.label.as_str()),
            };
            let Some(lines) = self.configured_agent_lines(&agent, row_cx.host) else {
                continue;
            };
            if selected && agent.focused() {
                focused = Some(count);
            }
            let gap = self.agent_row_gap(count);
            count += 1;
            list = list.child(self.agent_cell(index, agent, &row_cx, lines, gap, false, cx));
        }
        (list, count, focused)
    }

    /// Whether the agents' rows follow a configured `[sidebar_layout.agents]`.
    fn custom_agent_rows(&self) -> bool {
        self.config.usage.inline
            && self.config.sidebar_layout.agents != crate::config::AgentLayout::default()
    }

    /// The lines a configured agent row shows, empty for Herdr's own row, or
    /// None when the configured rows leave this agent out.
    pub(super) fn configured_agent_lines(
        &self,
        agent: &Listed<'_>,
        host: Option<&str>,
    ) -> Option<Vec<Vec<tokens::ResolvedToken>>> {
        if !self.custom_agent_rows() {
            return Some(Vec::new());
        }
        let layout = &self.config.sidebar_layout.agents;
        match agent {
            Listed::Herdr1 { agent, snapshot } => tokens::agent_rows(layout, agent, snapshot, host),
            Listed::Core(core) => Some(tokens::core_agent_rows(layout, core, host)),
        }
    }

    /// Space above the agent row painted `count`-th in its list: a
    /// configured `row_gap` between configured rows, none otherwise.
    pub(super) fn agent_row_gap(&self, count: usize) -> f32 {
        if self.custom_agent_rows() && count > 0 {
            f32::from(self.config.sidebar_layout.agents.row_gap) * line_height(&self.config.sidebar)
        } else {
            0.
        }
    }

    /// One agent's row, for the agents list, under its device's header, or
    /// `nested` under its workspace, where the row above already names the
    /// place: the agent of endpoint `index`, drawn in `row_cx`, that shows
    /// its pane when clicked.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn agent_cell(
        &self,
        index: usize,
        agent: Listed<'_>,
        row_cx: &RowContext,
        lines: Vec<Vec<tokens::ResolvedToken>>,
        gap: f32,
        nested: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let rows = layout_for(self.config.layout.mode);
        let multi = self.endpoints.len() > 1;
        let selected = index == self.selected_endpoint;
        let endpoint_id = self
            .endpoints
            .get(index)
            .map(|endpoint| endpoint.id.clone())
            .unwrap_or_default();
        let id = agent.id().to_owned();
        let target = agent.target();
        let navigate_endpoint = endpoint_id.clone();
        let menu = self.agent_menu_target(&endpoint_id, &agent);
        let menu_open = selected
            && match &agent {
                Listed::Herdr1 { .. } => self.pane_menu_target() == Some(id.as_str()),
                Listed::Core(_) => false,
            };
        let name = agent.name();
        let pane = agent.pane();
        let place = (!nested).then(|| agent.place()).flatten();
        let status = status_text(agent.status());
        let state = agent.state_label(&status);
        let spoken = crate::sidebar::accessibility::agent_name(
            name,
            pane.as_deref(),
            &state,
            place,
            row_cx.host,
        );
        let icon = agent.icon(&self.config.sidebar_style);
        let spoken = if icon == crate::icons::AgentIcon::Coordinator {
            fl!("sidebar-coordinator-row", row = spoken)
        } else {
            spoken
        };
        let focused = selected && agent.focused();
        Cell::new(
            rows,
            RowData::Agent(AgentRow {
                key: format!("agent-{id}"),
                name,
                icon,
                status: agent.status(),
                place,
                pane,
                status_text: self
                    .config
                    .sidebar_layout
                    .agents
                    .shows_status_text(agent.identity())
                    .then_some(state),
                lines,
            }),
            row_cx,
        )
        .selected(focused)
        .highlighted(menu_open)
        .row()
        .when(gap > 0., |row| row.mt(px(gap)))
        .id(SharedString::from(format!("agent-{endpoint_id}-{id}")))
        .role(Role::ListItem)
        .aria_label(spoken)
        .aria_selected(focused)
        .map(|control| self.chrome_stop(crate::window::chrome_focus::Region::Sidebar, control))
        .when(multi, |row| {
            row.debug_selector(|| format!("agent-{endpoint_id}-{id}"))
        })
        .when_some(menu, |row, menu| {
            row.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_listed_agent_menu(&menu, event.position, window, cx);
                }),
            )
        })
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_listed_agent(&navigate_endpoint, &target, window, cx);
        }))
    }

    /// The menu a right-click on a listed agent opens: a Herdr 1 agent's
    /// pane menu, or a Herdr 2 agent's pane menu in its tab. The shell shown
    /// in place of Herdr 1 has no tab, so no menu.
    fn agent_menu_target(&self, endpoint: &str, agent: &Listed<'_>) -> Option<AgentMenu> {
        match agent {
            Listed::Herdr1 { agent, .. } => Some(AgentMenu::Herdr1 {
                endpoint: endpoint.to_owned(),
                pane: agent.pane_id.clone(),
            }),
            Listed::Core(core) => {
                // The shell shown in place of Herdr 1 is in no tab.
                let tab = core.agent.tab.parse::<u64>().ok()?;
                (tab != crate::core_terminals::SHELL_TAB).then(|| AgentMenu::Core {
                    endpoint: endpoint.to_owned(),
                    target: agent.target(),
                })
            }
        }
    }

    fn open_listed_agent_menu(
        &mut self,
        menu: &AgentMenu,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match menu {
            AgentMenu::Herdr1 { endpoint, pane } => {
                self.open_agent_menu(endpoint, pane, position, window, cx);
            }
            AgentMenu::Core { endpoint, target } => {
                self.open_core_agent_menu(endpoint, target, position, window, cx);
            }
        }
    }
}

/// The menu a listed agent's right-click opens.
#[derive(Clone)]
enum AgentMenu {
    Herdr1 {
        endpoint: String,
        pane: String,
    },
    Core {
        endpoint: String,
        target: crate::sidebar::AgentTarget,
    },
}
