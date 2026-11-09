//! Herdr 2 agents on the Overview: the agents in the window's herdr-core
//! tabs, listed with this computer's Herdr 1 agents. herdr-core has no
//! workspaces or snapshot, so a row names its agent by identity
//! (`sheprd_core_client::agent_kind`), its checkout by folder, and opens the
//! tab and pane that hold it.

use super::{
    Error, Key, Open, Row, State,
    model::Since,
    replies::{BLOCKED, Presence, Reply, Target},
};
use crate::SheprdWindow;
use gpui::{Context, Window};
use sheprd_backend::TerminalId;
use sheprd_core_client::reply;
use std::time::{Instant, SystemTime};

/// What herdr-core agents are keyed by where a host's endpoint id would be:
/// pane ids of one Herdr and terminal ids of the other never meet.
pub(crate) const ENDPOINT: &str = "herdr-core";

/// Untrusted titles are cut here, as a Herdr 1 row's are.
const TITLE_LIMIT: usize = 200;

impl SheprdWindow {
    /// The endpoint and label of this computer, which the core runs on.
    fn core_overview_host(&self) -> (String, String) {
        self.endpoints
            .iter()
            .find(|endpoint| !endpoint.connection.target.is_remote())
            .map(|endpoint| (endpoint.id.clone(), endpoint.label.clone()))
            .unwrap_or_else(|| (ENDPOINT.to_owned(), crate::i18n::fl!("overview-core-host")))
    }

    /// Shows the pane of a core agent's row: its host and workspace first,
    /// which may be another than the window's, then its tab and pane.
    pub(super) fn open_core_overview_row(
        &mut self,
        tab: crate::browser::TabId,
        terminal: &TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = cx
            .try_global::<crate::browser::Store>()
            .and_then(|store| store.get(tab))
            .map(|shown| shown.workspace_id.clone());
        if let Some(workspace) = workspace {
            let (endpoint, _) = self.core_overview_host();
            self.navigate_endpoint(
                &endpoint,
                crate::NavigationTarget::Workspace(workspace.as_str()),
                cx,
            );
        }
        self.show_core_pane(tab, terminal, window, cx);
    }

    /// Whether `id` is the endpoint the core's rows are listed with.
    pub(super) fn is_core_overview_host(&self, id: &str) -> bool {
        self.core_overview_host().0 == id
    }

    /// The rows of the agents in the window's herdr-core tabs.
    pub(super) fn core_overview_rows(&self, since: &Since) -> Vec<Row> {
        let (endpoint, host) = self.core_overview_host();
        self.core_terminals
            .chat_terminals()
            .filter_map(|info| {
                let name = sheprd_core_client::agent_name(info)?;
                let tab = self.core_terminals.tab_of(&info.id)?;
                let folder = info.cwd.as_deref();
                let workspace = folder
                    .and_then(|cwd| cwd.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let checkout =
                    folder.map_or_else(|| info.id.to_string(), |cwd| cwd.display().to_string());
                let title = info
                    .title
                    .as_deref()
                    .map(|title| crate::notifications::safe_text(title, TITLE_LIMIT))
                    .map(|title| title.trim().to_owned())
                    .filter(|title| !title.is_empty() && *title != name)
                    .unwrap_or_else(|| name.clone());
                Some(Row {
                    key: Key::Agent {
                        endpoint: ENDPOINT.to_owned(),
                        checkout,
                        agent: name.clone(),
                    },
                    state: State::of(info.agent),
                    title,
                    detail: name.clone(),
                    identity: Some(name),
                    workspace,
                    branch: None,
                    host: host.clone(),
                    endpoint: endpoint.clone(),
                    since: since.get(ENDPOINT, info.id.as_str()),
                    pr: None,
                    open: Open::Core {
                        tab,
                        terminal: info.id.clone(),
                    },
                    muted: false,
                })
            })
            .collect()
    }

    /// Notes when each core agent entered its state, and the terminals
    /// listed, so agents gone from the core are forgotten.
    pub(super) fn observe_core_overview(&mut self, now: SystemTime) -> Vec<String> {
        let mut listed = Vec::new();
        for info in self.core_terminals.chat_terminals() {
            self.overview
                .since
                .observe_one(ENDPOINT, info.id.as_str(), info.agent, now);
            listed.push(info.id.to_string());
        }
        listed
    }
}

impl SheprdWindow {
    /// What a reply written to `agent` in herdr-core terminal `terminal`
    /// finds there now. The core has no boot to fence on: a terminal's
    /// address is never reused while it is listed, and the agent's name
    /// tells a terminal that has since started another program.
    pub(super) fn core_reply_presence(&self, terminal: &str, agent: &str) -> Presence {
        if self.core_terminals.hub.backend().is_none() {
            return Presence::Unreachable;
        }
        self.core_terminals
            .chat_terminals()
            .find(|info| info.id.as_str() == terminal)
            .filter(|info| sheprd_core_client::agent_name(info).as_deref() == Some(agent))
            .map_or(Presence::Gone, |info| Presence::Here(info.agent))
    }

    /// The agent a Herdr 2 row shows, as a reply dialog's target.
    pub(super) fn core_reply_target(&self, row: &Row, terminal: &TerminalId) -> Option<Target> {
        let info = self
            .core_terminals
            .chat_terminals()
            .find(|info| info.id == *terminal)?;
        Some(Target {
            title: row.title.clone(),
            endpoint: ENDPOINT.to_owned(),
            pane: terminal.to_string(),
            boot: String::new(),
            agent: sheprd_core_client::agent_name(info)?,
        })
    }

    /// Types `reply` into its terminal through the core, off the UI thread.
    /// A blocked agent is refused by the core itself, as Herdr 1's daemon
    /// refuses one, and the reply waits again.
    pub(super) fn send_core_reply(&mut self, reply: Reply, cx: &mut Context<Self>) {
        let backend = self.core_terminals.hub.backend().cloned();
        let id = reply.id;
        let work = cx.background_executor().spawn(async move {
            let backend = backend.ok_or(Error::ReplyCore(sheprd_core_client::Error::Lost))?;
            // Checked again against the core as it goes: the list the
            // presence came from is a few seconds old, and the terminal
            // takes text whatever program has it.
            let replied = reply::reply(backend.core(), &reply.pane, &reply.agent, &reply.text)
                .map_err(|error| match error {
                    sheprd_core_client::Error::Refused {
                        kind: Some(kind),
                        message,
                        ..
                    } if kind == sheprd_core_client::wire::BLOCKED
                        || kind == sheprd_core_client::wire::STATUS_MISMATCH =>
                    {
                        // Nothing was typed; it waits for the agent again.
                        Error::ReplyRefused {
                            code: BLOCKED.to_owned(),
                            message,
                        }
                    }
                    error => Error::ReplyCore(error),
                })?;
            match replied {
                reply::Replied::Sent => Ok(()),
                reply::Replied::Gone => Err(Error::ReplyGone),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| {
                this.overview.replies.finish(id, result, Instant::now());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
