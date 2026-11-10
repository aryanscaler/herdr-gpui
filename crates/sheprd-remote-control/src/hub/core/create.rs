//! New work from a phone. While a core answers, new tabs and chats start
//! here and never on Herdr 1: a failure reaches the phone as it is, rather
//! than quietly landing the work on the other generation.
//!
//! A chat is `sheprd-acp` as the terminal's program, as the desktop starts
//! one: the core keeps no pane metadata a program could report through, so
//! the endpoint is chosen here and kept as a label.

use super::{Error, HubError, Result, Source, lock, pane_id, worktree};
use crate::hub::Layout;
use serde_json::{Value, json};
use sheprd_chat_protocol::{Agent, ENDPOINT_ENV, EndpointId, TOKEN};
use sheprd_core_client::wire::Run;
use sheprd_session::{Labelled, NEW_WORKSPACE_LABEL, SESSION_LABEL, WORKSPACE_LABEL};

/// Where new work from a phone belongs, which Sheprd gives it a tab in.
#[derive(Clone, Copy, Debug)]
pub(in crate::hub) enum Place<'a> {
    /// A workspace that exists.
    Workspace(Labelled<'a>),
    /// A new workspace in this folder, which Sheprd keeps.
    New(&'a str),
}
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

/// The variable naming the Herdr pane `sheprd-acp` reports to. A core
/// started from a Herdr pane passes that pane on; a chat must not report
/// itself there.
const PANE_ENV: &str = "HERDR_PANE_ID";

impl Source {
    /// A shell in `cwd`, or the core's default folder when it is empty,
    /// labelled for `workspace` so Sheprd puts it in a tab there.
    pub(in crate::hub) fn open_shell(&self, cwd: &str, place: Option<Place<'_>>) -> Result<String> {
        self.run(&Run {
            cwd: Some(cwd).filter(|cwd| !cwd.is_empty()),
            keep: true,
            labels: place_labels(place),
            ..Run::default()
        })
    }

    /// A chat running `agent` in `cwd`, listed once the next poll sees it.
    pub(in crate::hub) fn open_chat(
        &self,
        agent: Agent,
        program: &Path,
        cwd: &str,
        place: Option<Place<'_>>,
    ) -> Result<String> {
        let endpoint = EndpointId::random().to_string();
        let mut labels = place_labels(place);
        labels.insert(TOKEN, endpoint.as_str());
        let argv = [program.to_string_lossy().into_owned(), agent.to_string()];
        self.run(&Run {
            argv: &argv,
            cwd: Some(cwd).filter(|cwd| !cwd.is_empty()),
            env: BTreeMap::from([(ENDPOINT_ENV, endpoint.as_str()), (PANE_ENV, "")]),
            // A recovery shell cannot serve the labelled chat endpoint.
            // Let a lost chat finish instead of advertising a dead chat.
            keep: false,
            labels,
            ..Run::default()
        })
    }

    /// A chat as [`Self::open_chat`], in a new worktree of `cwd` on `branch`
    /// under `root`. Answers its pane and the new checkout's folder.
    pub(in crate::hub) fn open_worktree_chat(
        &self,
        agent: Agent,
        program: &Path,
        cwd: &str,
        branch: &str,
        root: Option<&Path>,
    ) -> Result<(String, String)> {
        let root = root.ok_or(worktree::Error::NoHome).map_err(Error::from)?;
        let repository = Path::new(cwd);
        let checkout =
            worktree::add(repository, branch, "HEAD", root, || false).map_err(Error::from)?;
        // A new workspace of its own, in the new checkout.
        let checkout = checkout.to_string_lossy().into_owned();
        let started = self.open_chat(agent, program, &checkout, Some(Place::New(&checkout)));
        let folder = Path::new(&checkout);
        if let Err(error) = &started {
            // A retry must find the branch free, but only a start that
            // surely ran nothing may go: a chat may be working in the
            // checkout after any other failure.
            if !ran_nothing(error) {
                tracing::warn!(%error, %branch, "A worktree stayed: its chat may have started");
            } else if let Err(error) = worktree::remove(repository, folder, branch) {
                tracing::warn!(%error, %branch, "A worktree without a chat stayed");
            }
        }
        started.map(|pane| (pane, checkout))
    }

    fn run(&self, run: &Run<'_>) -> Result<String> {
        let terminal = self.start_run(run)?;
        let state = lock(&self.state);
        let core = state.core.as_ref().ok_or(Error::Disconnected)?;
        Ok(pane_id(&core.hello().core.id, &terminal.address))
    }

    /// Starts `run`, listed here at once: input may arrive before the next
    /// poll lists it.
    pub(super) fn start_run(&self, run: &Run<'_>) -> Result<sheprd_core_client::wire::Terminal> {
        let core = lock(&self.state).core.clone();
        let core = core.ok_or(Error::Disconnected)?;
        let terminal = core.run(run)?;
        lock(&self.state).terminals.push(terminal.clone());
        Ok(terminal)
    }

    /// Ends the terminals `target` names: a pane's, or every pane's of a
    /// tab, chats included, as closing a Herdr 1 tab closes them all. The
    /// desktop drops a pane whose program ended, and its tab with the last.
    pub(super) fn close(&self, target: &str) -> Result<Value> {
        let (core, addresses) = {
            let state = lock(&self.state);
            let core = state.core.clone().ok_or(Error::Disconnected)?;
            let boot = &core.hello().core.id;
            let panes = state
                .tabs
                .get(target)
                .cloned()
                .unwrap_or_else(|| vec![target.to_owned()]);
            let addresses: Vec<String> = state
                .terminals
                .iter()
                .map(|terminal| terminal.address.clone())
                .filter(|address| panes.contains(&pane_id(boot, address)))
                .collect();
            if addresses.is_empty() {
                return Err(Error::Unsupported.into());
            }
            (core, addresses)
        };
        // Each one ended is gone from the list, even if a later one fails.
        for address in addresses {
            core.kill(&address)?;
            let mut state = lock(&self.state);
            state.screens.remove(&address);
            state
                .terminals
                .retain(|terminal| terminal.address != address);
        }
        Ok(json!({}))
    }
}

/// Whether a failed start surely started no terminal: the core refused it,
/// or there was no core to ask. A timeout or lost connection may come after
/// the core acted on the request.
pub(super) fn ran_nothing(error: &HubError) -> bool {
    matches!(
        error,
        HubError::Core(
            Error::Disconnected | Error::Client(sheprd_core_client::Error::Refused { .. })
        )
    )
}

/// The panes of each tab in `layout`. A chat is listed apart from its tab's
/// terminals, so a tab of a chat and a shell still counts as two.
pub(super) fn tab_panes(layout: &Layout) -> HashMap<String, Vec<String>> {
    let mut tabs: HashMap<String, Vec<String>> = HashMap::new();
    for workspace in &layout.workspaces {
        for terminal in &workspace.terminals {
            let panes = terminal.panes.iter().map(|pane| pane.pane.clone());
            tabs.entry(terminal.tab.clone()).or_default().extend(panes);
        }
        for chat in &workspace.chats {
            let panes = tabs.entry(chat.tab.clone()).or_default();
            panes.push(chat.pane.clone());
        }
    }
    tabs
}

/// The labels naming where `place` is, when it is somewhere Sheprd could
/// keep a tab.
fn place_labels(place: Option<Place<'_>>) -> BTreeMap<&'static str, &str> {
    match place {
        Some(Place::Workspace(workspace)) if sheprd_session::tab_workspace(workspace.id) => {
            let mut labels = BTreeMap::from([(WORKSPACE_LABEL, workspace.id)]);
            if let Some(session) = workspace.session {
                labels.insert(SESSION_LABEL, session);
            }
            labels
        }
        Some(Place::New(cwd)) if Path::new(cwd).is_absolute() => {
            BTreeMap::from([(NEW_WORKSPACE_LABEL, cwd)])
        }
        _ => BTreeMap::new(),
    }
}
