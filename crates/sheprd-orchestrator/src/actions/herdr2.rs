//! A dispatch on Herdr 2: herdr-core keeps no workspaces or worktrees, so
//! `sheprd_remote::work` makes the checkout with Git, in the folder Herdr 1
//! would use, and types the agent into a herdr-core shell there. The run
//! records that shell's terminal, which Stop, Send, and Remove then address.

use super::{
    super::model::{Pane, Workspace},
    Attempt, Destination, DispatchRequest, place,
};
use crate::{Backend, Error, HerdrSession, Result, Run, RunState, Store, error::script};
use chrono::Utc;
use sheprd_client::shell_quote;
use sheprd_remote::{
    teleport::Host,
    work::{self, NewWorktree, Started, Worktree},
};
use std::sync::atomic::AtomicBool;

#[derive(serde::Deserialize)]
struct Listed {
    terminals: Vec<Address>,
}

#[derive(serde::Deserialize)]
struct Address {
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    state: TerminalState,
}

#[derive(Default, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TerminalState {
    Live,
    Finished,
    #[default]
    #[serde(other)]
    Other,
}

fn terminals(host: &Host, cancelled: &AtomicBool) -> Result<Listed> {
    let output = host
        .capture("core_cli ls --all --json\n", cancelled)
        .map_err(script("Reading the agent terminals"))?;
    serde_json::from_slice(&output).map_err(|source| Error::OutputJson {
        operation: "Reading the agent terminals",
        source,
    })
}

/// A failed start may have left a shell whose address the work API could
/// not return. Never delete its checkout or guess that shell's ownership.
pub(super) fn unused_checkout(
    host: &Host,
    workspace: &Workspace,
    cancelled: &AtomicBool,
) -> Result<()> {
    let Some(path) = workspace.path.as_deref() else {
        return Ok(());
    };
    let listed = terminals(host, cancelled)?;
    if listed.terminals.iter().any(|terminal| {
        terminal.state != TerminalState::Finished
            && (terminal.cwd.is_empty() || std::path::Path::new(&terminal.cwd).starts_with(path))
    }) {
        return Err(Error::InvalidCheckout);
    }
    work::owned_agent::checkout_idle(
        host,
        &Worktree {
            path: path.to_string_lossy().into_owned(),
            opened: work::Opened::Herdr2,
        },
        cancelled,
    )?;
    Ok(())
}

/// Makes the run's worktree from `checkout`, starts its agent there, and
/// sends it the prompt once it is ready, recording each step in `store`.
/// The worktree made is left in `created` for the caller to undo on failure;
/// an agent that started is recorded before its prompt so the caller can
/// confirm it ended before removing its checkout on failure.
#[allow(clippy::too_many_arguments)]
pub(super) fn launch(
    destination: &Destination,
    checkout: &str,
    request: &DispatchRequest,
    name: &str,
    run: &mut Run,
    store: &mut Store,
    attempt: &mut Attempt,
    cancelled: &AtomicBool,
) -> Result<()> {
    let host = &destination.host;
    let new = NewWorktree {
        branch: &request.branch,
        base: &destination.commit,
        label: None,
    };
    let worktree = work::owned_checkout::plan(host, checkout, &request.branch, cancelled)?;
    // Creation itself can lose its reply. Record its planned path first and
    // never delete a path without the successful-creation receipt.
    attempt.created = None;
    let (ssh, session) = place(&destination.target);
    run.workspace = Some(Workspace {
        backend: Backend::Herdr,
        // No Herdr workspace: the app keeps one when the run is opened.
        id: String::new(),
        host: ssh.clone(),
        path: Some(worktree.path.clone().into()),
        branch: request.branch.clone(),
    });
    run.state = RunState::Starting;
    run.updated_at = Utc::now();
    let pending = HerdrSession {
        checkout_token: Some(run.id.clone()),
        adopted: None,
        run_id: run.id.clone(),
        target: Some(destination.target.clone()),
        host: ssh.clone(),
        session: session.clone(),
        pane: Pane::Pending,
        agent_name: name.to_owned(),
        updated_at: run.updated_at,
    };
    store.update_run(run, Some(&pending))?;
    work::owned_checkout::create(host, checkout, &worktree, &new, &run.id, cancelled)?;
    let agent = work::Agent {
        kind: request.kind,
        name,
        model: request.model.as_deref(),
        prompt: None,
    };
    // From the launch intent onward, a lost reply can mean the agent is
    // already working. Keep its checkout and receipt; never replay a prompt.
    attempt.created = None;
    let started = work::owned_agent::start(host, &worktree, &agent, owner(name), cancelled)?;
    let Started::Herdr2 { terminal } = &started else {
        return Err(Error::Output {
            operation: "Starting the agent",
        });
    };
    let herdr_session = HerdrSession {
        checkout_token: Some(run.id.clone()),
        adopted: None,
        run_id: run.id.clone(),
        target: Some(destination.target.clone()),
        host: ssh,
        session,
        pane: Pane::Herdr2 {
            terminal: terminal.clone(),
        },
        agent_name: name.to_owned(),
        updated_at: Utc::now(),
    };
    attempt.retained_session = Some(herdr_session.clone());
    prompt(
        host,
        &started,
        request,
        run,
        store,
        &herdr_session,
        cancelled,
    )
}

pub(super) fn owner(token: &str) -> work::owned_agent::Owner<'_> {
    work::owned_agent::Owner {
        key: "sheprd.orchestrator.run",
        value: token,
    }
}

pub(super) fn recover(
    host: &Host,
    run: &Run,
    session: &HerdrSession,
    cancelled: &AtomicBool,
) -> Result<Started> {
    inspect(host, run, session, false, cancelled)
}

pub(super) fn inspect(
    host: &Host,
    run: &Run,
    session: &HerdrSession,
    include_finished: bool,
    cancelled: &AtomicBool,
) -> Result<Started> {
    if let Some(receipt) = &session.adopted {
        return super::legacy::verify(host, receipt, include_finished, cancelled);
    }
    let workspace = run.workspace.as_ref().ok_or(Error::NothingToOpen)?;
    let path = workspace
        .path
        .as_ref()
        .ok_or(Error::NothingToOpen)?
        .to_string_lossy();
    let tree = Worktree {
        path: path.into_owned(),
        opened: work::Opened::Herdr2,
    };
    let agent = work::Agent {
        kind: sheprd_remote::teleport::AgentKind::parse(&run.agent).ok_or(Error::NothingToOpen)?,
        name: &session.agent_name,
        model: run.model.as_deref(),
        prompt: None,
    };
    let expected = match &session.pane {
        Pane::Herdr2 { terminal } => Some(terminal.as_str()),
        Pane::Pending => None,
        Pane::Herdr1 { .. } => return Err(Error::NothingToOpen),
    };
    Ok(work::owned_agent::inspect(
        host,
        &tree,
        owner(&session.agent_name),
        &agent,
        expected,
        include_finished,
        cancelled,
    )?)
}

fn prompt(
    host: &Host,
    started: &Started,
    request: &DispatchRequest,
    run: &mut Run,
    store: &mut Store,
    herdr_session: &HerdrSession,
    cancelled: &AtomicBool,
) -> Result<()> {
    store.update_run(run, Some(herdr_session))?;
    work::prompt(host, started, &request.prompt, cancelled)?;
    run.state = RunState::Running;
    run.updated_at = Utc::now();
    store.update_run(run, Some(herdr_session))
}

/// Interrupts the agent in `terminal` as Herdr 1's Stop does: Escape, then
/// Ctrl-C.
pub(super) fn stop(host: &Host, terminal: &str, cancelled: &AtomicBool) -> Result<()> {
    let line = format!(
        "core_cli keys --json -- {} esc ctrl+c >/dev/null\n",
        shell_quote(terminal)
    );
    host.capture(&line, cancelled)
        .map(drop)
        .map_err(script("Stopping the agent"))
}

/// Types `text` to the agent in `terminal` as a prompt, as Herdr 1's
/// `agent prompt` does, without waiting for it to finish what it is doing.
pub(super) fn send(host: &Host, terminal: &str, text: &str, cancelled: &AtomicBool) -> Result<()> {
    let line = format!(
        "core_cli send --json -- {} {} >/dev/null\n",
        shell_quote(terminal),
        shell_quote(text)
    );
    host.capture(&line, cancelled)
        .map(drop)
        .map_err(script("Sending the message"))
}

/// Ends the shell in `terminal` and the agent in it, as removing Herdr 1's
/// workspace closes its panes. One already gone is no failure.
pub(super) fn end(host: &Host, terminal: &str) -> Result<()> {
    work::owned_agent::end_verified(host, terminal, &AtomicBool::new(false))?;
    Ok(())
}
