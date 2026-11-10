//! The review phase: read the source workspace and the destination, and
//! decide what each pane becomes.
use super::{Candidate, Destination, Keeping, Source};
use crate::teleport::{
    core, credentials,
    error::{Error, Result, Step},
    git,
    host::Host,
    launch::{AgentKind, Work, handoff_path},
    layout::{Node, tree},
    provision::Arrival,
    remote::MatchReason,
    snapshot::{HostSnapshot, ProcessInfo, ProcessInfoResult, SnapshotResult},
};
use sheprd_backend::Generation;
use sheprd_client::shell_quote;
use sheprd_session::{Keeper, panes::MAX_PANES};
use std::{collections::HashMap, sync::atomic::AtomicBool};

/// What the destination pane will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// An idle shell.
    Shell,
    /// Move the agent's session and resume it.
    Resume(AgentKind),
    /// Ask the source agent for a note, then start `to` with it.
    Handoff { note: String, to: AgentKind },
    /// Start the same program afresh.
    Start(Vec<String>),
    /// Run the same command again.
    Run(Vec<String>),
    /// Nothing on the destination can take this over.
    Missing(String),
}

#[derive(Debug, Clone)]
pub struct PanePlan {
    pub pane_id: String,
    /// The directory the source program runs in.
    pub cwd: Option<String>,
    pub work: Work,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub struct TabPlan {
    /// Which Herdr runs the source tab's terminals, and so how its agents
    /// are asked for notes and its programs stopped.
    pub generation: Generation,
    /// Which Herdr runs it on the destination; see `destination_generation`.
    pub into: Generation,
    pub label: Option<String>,
    pub tree: Node,
    pub panes: Vec<PanePlan>,
}

impl TabPlan {
    pub(super) fn pane(&self, id: &str) -> Option<&PanePlan> {
        self.panes.iter().find(|pane| pane.pane_id == id)
    }
}

#[derive(Debug, Clone)]
pub struct Review {
    pub checkout: String,
    pub branch: String,
    pub state: git::SourceState,
    pub tabs: Vec<TabPlan>,
    /// Why an open repository was chosen; `None` when it arrives fresh.
    pub reason: Option<MatchReason>,
    pub github: GitHubAccess,
}

/// Whether the destination can pull and push to GitHub by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubAccess {
    /// It reaches `origin` already, or `origin` is not on GitHub.
    Direct,
    /// It cannot: this machine's `gh` token is installed for the repository.
    Token,
    /// It cannot, and `gh` here is not signed in to lend a token.
    Unavailable,
}

impl Review {
    pub fn panes(&self) -> impl Iterator<Item = &PanePlan> {
        self.tabs.iter().flat_map(|tab| tab.panes.iter())
    }
}

pub(in crate::teleport) fn process_infos(
    host: &Host,
    panes: &[String],
    cancelled: &AtomicBool,
) -> Result<HashMap<String, ProcessInfo>> {
    let body = panes
        .iter()
        .map(|pane| {
            format!(
                "printf '\\036%s\\n' {pane}\nherdr_cli pane process-info --pane {pane} 2>/dev/null || :\n",
                pane = shell_quote(pane)
            )
        })
        .collect::<String>();
    let output = host.query(Step::Review, &body, &[], cancelled)?;
    let output = String::from_utf8_lossy(&output);
    Ok(output
        .split('\u{1e}')
        .filter_map(|section| {
            let (pane, json) = section.split_once('\n')?;
            let info =
                serde_json::from_str::<crate::teleport::snapshot::Envelope<ProcessInfoResult>>(
                    json,
                )
                .ok()?
                .result
                .process_info;
            Some((pane.to_owned(), info))
        })
        .collect())
}

/// Choose what each pane becomes on a destination with `installed` programs.
pub(in crate::teleport) fn plan_action(
    work: &Work,
    installed: &[String],
    note: impl FnOnce() -> String,
) -> Action {
    let has = |program: &str| installed.iter().any(|p| p == program);
    let fallback = || {
        AgentKind::FALLBACKS
            .into_iter()
            .find(|kind| has(kind.binary()))
    };
    match work {
        Work::Shell => Action::Shell,
        Work::Session { agent, .. } if has(agent.binary()) => Action::Resume(*agent),
        Work::Session { agent, .. } => fallback().map_or_else(
            || Action::Missing(agent.binary().to_owned()),
            |to| Action::Handoff { note: note(), to },
        ),
        Work::Agent { name, argv } => match AgentKind::parse(name) {
            Some(kind) if kind.takes_prompt() && has(kind.binary()) => Action::Handoff {
                note: note(),
                to: kind,
            },
            _ if work.program().is_some_and(has) => Action::Start(argv.clone()),
            _ => fallback().map_or_else(
                || Action::Missing(name.clone()),
                |to| Action::Handoff { note: note(), to },
            ),
        },
        Work::Command(argv) => Action::Run(argv.clone()),
    }
}

/// Read the source workspace and the destination, and decide what moves.
pub fn review(source: &Source, destination: &Candidate, cancelled: &AtomicBool) -> Result<Review> {
    let host = &source.place.host;
    // A workspace the app keeps has no Herdr 1 tabs, and Herdr 1 may not
    // even be installed; its folder, perhaps a subfolder, names the checkout.
    let (snapshot, checkout) = match &source.keeping {
        Keeping::App { checkout } => (
            HostSnapshot::default(),
            git::toplevel(host, checkout, Step::Review, cancelled)?,
        ),
        Keeping::Herdr1 => {
            let snapshot: SnapshotResult =
                host.herdr(Step::Review, &["api", "snapshot"], cancelled)?;
            let snapshot = snapshot.snapshot;
            let checkout = snapshot
                .workspace(&source.workspace_id)
                .and_then(|workspace| workspace.worktree.as_ref())
                .filter(|worktree| worktree.is_linked_worktree)
                .map(|worktree| worktree.checkout_path.clone())
                .ok_or(Error::WorkspaceGone)?;
            (snapshot, checkout)
        }
    };
    let state = git::source_state(host, &checkout, cancelled)?;
    let branch = state.branch.clone().ok_or(Error::DetachedHead)?;
    // A fresh clone has no branch to collide with; anything already there does.
    let key = match &destination.destination {
        Destination::Open { repository, .. } => Some(repository.key.as_str()),
        Destination::Arrive(Arrival::Existing { key, .. }) => Some(key.as_str()),
        Destination::Arrive(Arrival::Clone { .. }) => None,
        // The checkout there is this branch's, and is backed up before reuse.
        Destination::Reclaim { .. } => None,
    };
    if let Some(key) = key {
        let dest = git::destination_branch(&destination.place.host, key, &branch, cancelled)?;
        git::check_destination(host, &checkout, &branch, &dest, cancelled)?;
    }

    let tabs = snapshot.tabs_of(&source.workspace_id);
    let pane_ids: Vec<String> = snapshot
        .panes
        .iter()
        .filter(|pane| tabs.iter().any(|tab| tab.tab_id == pane.tab_id))
        .map(|pane| pane.pane_id.clone())
        .collect();
    let processes = process_infos(host, &pane_ids, cancelled)?;
    let mut works: Vec<(String, Work, Option<String>)> = pane_ids
        .iter()
        .filter_map(|id| {
            let pane = snapshot.pane(id)?;
            let process = processes.get(id).cloned().unwrap_or_default();
            let cwd = process
                .foreground_job()
                .and_then(|job| job.cwd.clone())
                .or_else(|| pane.foreground_cwd.clone())
                .or_else(|| pane.cwd.clone());
            Some((id.clone(), Work::classify(pane, &process), cwd))
        })
        .collect();
    // The app's Herdr 2 tabs in the workspace, read from the host's core.
    let core_terminals = core::terminals(host, &core::ids(&source.core_tabs), cancelled)?;
    // Sessions that cannot be read leave their agents to hand off.
    let sessions = match core::sessions(host, &core_terminals, cancelled) {
        Ok(sessions) => sessions,
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(error) => {
            tracing::warn!(%error, "Could not read the sessions of Herdr 2 agents");
            HashMap::new()
        }
    };
    works.extend(core::ids(&source.core_tabs).into_iter().map(|id| {
        let terminal = core_terminals.get(&id).cloned().unwrap_or_default();
        let (work, cwd) = (core::work(&terminal, sessions.get(&id)), terminal.cwd());
        (id, work, cwd)
    }));
    let mut programs: Vec<&str> = AgentKind::FALLBACKS.iter().map(|k| k.binary()).collect();
    programs.extend(works.iter().filter_map(|(_, work, _)| work.program()));
    programs.sort_unstable();
    programs.dedup();
    let installed = destination
        .place
        .host
        .installed(Step::Review, &programs, cancelled)?;

    let github = match destination
        .origin
        .as_deref()
        .filter(|origin| credentials::is_github(origin))
    {
        Some(origin) if !credentials::reachable(&destination.place.host, origin, cancelled)? => {
            if credentials::local_token(cancelled)?.is_some() {
                GitHubAccess::Token
            } else {
                GitHubAccess::Unavailable
            }
        }
        _ => GitHubAccess::Direct,
    };

    // Where the app keeps the workspaces, every tab moves into Herdr 2.
    let kept = destination.lands_in == Keeper::App;
    let wants_core = kept || source.core_preferred || !source.core_tabs.is_empty();
    let core_there = wants_core && core::available(&destination.place.host, cancelled)?;
    if kept && !core_there {
        return Err(Error::CoreMissing {
            host: destination.place.label.clone(),
        });
    }
    let preferred = kept || source.core_preferred;
    let into = |generation| destination_generation(generation, core_there, preferred);
    let mut notes = 0;
    let mut planned: Vec<TabPlan> = tabs
        .iter()
        .map(|tab| {
            let node = tab_tree(&snapshot, tab);
            let label = source.tab_labels.get(&tab.tab_id).cloned();
            let moves = (Generation::Herdr1, into(Generation::Herdr1));
            plan_tree(moves, kept, label, node, &works, &installed, &mut notes)
        })
        .collect::<Result<_>>()?;
    for panes in &source.core_tabs {
        let moves = (Generation::Herdr2, into(Generation::Herdr2));
        let tree = core::tree(panes);
        planned.push(plan_tree(
            moves, kept, None, tree, &works, &installed, &mut notes,
        )?);
    }
    Ok(Review {
        checkout,
        branch,
        state,
        tabs: planned,
        reason: match &destination.destination {
            Destination::Open { reason, .. } => Some(*reason),
            Destination::Arrive(_) | Destination::Reclaim { .. } => None,
        },
        github,
    })
}

/// Which Herdr a tab run by `generation` here runs in on the destination.
/// A Herdr 2 tab stays in Herdr 2, and with Herdr 2 `preferred` for new
/// tabs every tab moves into it, wherever the destination runs a core;
/// without one, every tab goes to its Herdr 1.
fn destination_generation(generation: Generation, core_there: bool, preferred: bool) -> Generation {
    if core_there && (preferred || generation == Generation::Herdr2) {
        Generation::Herdr2
    } else {
        Generation::Herdr1
    }
}

/// A Herdr 1 tab's split tree, or its panes in a row when its layout
/// cannot be read.
fn tab_tree(snapshot: &HostSnapshot, tab: &crate::teleport::snapshot::Tab) -> Node {
    let members: Vec<String> = snapshot
        .panes
        .iter()
        .filter(|pane| pane.tab_id == tab.tab_id)
        .map(|pane| pane.pane_id.clone())
        .collect();
    snapshot
        .layout(&tab.tab_id)
        .and_then(tree)
        .or_else(|| Node::row(&members))
        .unwrap_or_else(|| Node::Pane(String::new()))
}

/// What each pane of one source tab, laid out as `node`, becomes, as it
/// moves `from` one Herdr `into` one on the destination. A tab with more
/// panes than a Herdr 2 tab holds stays in Herdr 1, unless it lands in a
/// workspace the app `kept`, which holds only Herdr 2 tabs: there the move
/// is refused before anything changes.
fn plan_tree(
    (generation, into): (Generation, Generation),
    kept: bool,
    label: Option<String>,
    node: Node,
    works: &[(String, Work, Option<String>)],
    installed: &[String],
    notes: &mut usize,
) -> Result<TabPlan> {
    let slots = node.plan().slots;
    if kept && slots.len() > MAX_PANES {
        return Err(Error::TabTooLarge {
            panes: slots.len(),
            most: MAX_PANES,
        });
    }
    let into = if slots.len() > MAX_PANES {
        Generation::Herdr1
    } else {
        into
    };
    let panes = slots
        .into_iter()
        .filter_map(|id| {
            let (_, work, cwd) = works.iter().find(|(pane, ..)| *pane == id)?;
            let action = plan_action(work, installed, || {
                let path = handoff_path(*notes);
                *notes += 1;
                path
            });
            Some(PanePlan {
                pane_id: id,
                cwd: cwd.clone(),
                work: work.clone(),
                action,
            })
        })
        .collect();
    Ok(TabPlan {
        generation,
        into,
        label,
        tree: node,
        panes,
    })
}

#[cfg(test)]
mod tests;
