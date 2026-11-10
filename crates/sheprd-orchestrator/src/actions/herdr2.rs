//! A dispatch on Herdr 2: herdr-core keeps no workspaces or worktrees, so
//! `sheprd_remote::work` makes the checkout with Git, in the folder Herdr 1
//! would use, and types the agent into a herdr-core shell there. The run
//! records that shell's terminal, which Stop, Send, and Remove then address.

use super::{
    super::model::{Pane, Workspace},
    Destination, DispatchRequest, place,
};
use crate::{Backend, Error, HerdrSession, Result, Run, RunState, Store, error::script};
use chrono::Utc;
use sheprd_client::shell_quote;
use sheprd_remote::{
    teleport::Host,
    work::{self, NewWorktree, Repository, Started, Worktree},
};
use std::sync::atomic::AtomicBool;

/// Makes the run's worktree from `checkout`, starts its agent there, and
/// sends it the prompt once it is ready, recording each step in `store`.
/// The worktree made is left in `created` for the caller to undo on failure;
/// an agent that started but failed to take its prompt is ended here.
#[allow(clippy::too_many_arguments)]
pub(super) fn launch(
    destination: &Destination,
    checkout: &str,
    request: &DispatchRequest,
    name: &str,
    run: &mut Run,
    store: &mut Store,
    created: &mut Option<(Host, Worktree)>,
    cancelled: &AtomicBool,
) -> Result<()> {
    let host = &destination.host;
    let new = NewWorktree {
        branch: &request.branch,
        base: &destination.commit,
        label: None,
    };
    let repository = Repository::Herdr2 {
        checkout: checkout.to_owned(),
    };
    let worktree = work::create(host, &repository, &new, cancelled)?;
    *created = Some((host.clone(), worktree.clone()));
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
    store.update_run(run, None)?;
    let agent = work::Agent {
        kind: request.kind,
        name,
        model: request.model.as_deref(),
        prompt: None,
    };
    let started = work::start_agent(host, &worktree, &agent, cancelled)?;
    let Started::Herdr2 { terminal } = &started else {
        return Err(Error::Output {
            operation: "Starting the agent",
        });
    };
    let herdr_session = HerdrSession {
        run_id: run.id.clone(),
        host: ssh,
        session,
        pane: Pane::Herdr2 {
            terminal: terminal.clone(),
        },
        agent_name: name.to_owned(),
        updated_at: Utc::now(),
    };
    let outcome = prompt(
        host,
        &started,
        request,
        run,
        store,
        &herdr_session,
        cancelled,
    );
    if outcome.is_err() {
        // Its worktree goes next; an agent left typing in it would not.
        end(host, terminal);
    }
    outcome
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
pub(super) fn end(host: &Host, terminal: &str) {
    let line = format!(
        "core_cli kill --json -- {} >/dev/null 2>&1 || true\n",
        shell_quote(terminal)
    );
    if let Err(error) = host.capture(&line, &AtomicBool::new(false)) {
        tracing::debug!(category = "orchestrator", %error, "Cannot end a run's terminal");
    }
}
