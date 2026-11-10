//! What a herdr-core tab shows in the tab strip beside its title, as a
//! Herdr 1 tab does: its agents' status and the logo of the agent running
//! in it, the zoom mark while a pane fills it, and the muted mark.

use super::Host;
use crate::{
    SheprdWindow,
    browser::{Location, Tab},
    icons::AgentIcon,
    tab_layout::{Lead, Mark},
};
use gpui::App;
use sheprd_backend::TerminalId;
use sheprd_client::protocol::AgentStatus;

/// A herdr-core tab's face in the strip.
pub(crate) struct CoreFace {
    /// Its agents' status and logo, while one runs in it.
    pub(crate) mark: Option<Mark>,
    pub(crate) zoomed: bool,
    pub(crate) muted: bool,
}

impl CoreFace {
    /// What leads the tab's title.
    pub(crate) fn lead(&self) -> Lead {
        self.mark.as_ref().map_or(Lead::Icon, Mark::lead)
    }
}

impl SheprdWindow {
    /// The face of `tab` when it shows herdr-core terminals; none for a page.
    pub(crate) fn core_face(&self, tab: &Tab, cx: &App) -> Option<CoreFace> {
        let Some(Location::CoreTerminal {
            panes,
            active,
            zoomed,
            ..
        }) = &tab.location
        else {
            return None;
        };
        let host = Host::of(&tab.scope);
        let status = self.core_terminals.tab_status(&host, panes);
        // The window's own layout, when it shows the tab, is newer than what
        // was saved.
        let shown = self.core_terminals.tabs.get(&tab.id);
        let active = shown.map(|shown| &shown.active).or(active.as_ref());
        let zoomed = match shown {
            Some(shown) => shown.zoom.is_open() && shown.zoomed.is_some(),
            None => zoomed.is_some(),
        };
        let mark = (status != AgentStatus::Unknown).then(|| Mark::Herdr {
            status,
            agent: self.core_agent_icon(&host, active, &panes.terminals()),
        });
        Some(CoreFace {
            mark,
            zoomed,
            muted: self.core_tab_silenced(tab.id, cx),
        })
    }

    /// The logo of the agent a tab's panes run: the pane in use's when it
    /// runs one, else the first that does.
    fn core_agent_icon(
        &self,
        host: &Host,
        active: Option<&TerminalId>,
        terminals: &[&TerminalId],
    ) -> Option<AgentIcon> {
        let infos = self.core_terminals.infos_of(host)?;
        let kind =
            |terminal: &TerminalId| infos.get(terminal).and_then(sheprd_core_client::agent_kind);
        active
            .and_then(kind)
            .or_else(|| terminals.iter().find_map(|terminal| kind(terminal)))
            .map(|kind| AgentIcon::from_identity(Some(kind.name())))
    }
}
