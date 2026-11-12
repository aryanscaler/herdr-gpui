//! Recoverable owned agents. The ownership token is attached by `run` itself,
//! so losing its reply does not lose the terminal's ownership identity.
//! Recovery only reads: an absent receipt never authorizes another launch.

use super::{Agent, Host, Opened, Started, Step, Worktree, script};
use sheprd_client::shell_quote;
use sheprd_core_client::wire::{List, TerminalState};
use std::sync::atomic::AtomicBool;

mod end;
mod error;
pub use end::{checkout_idle, end, end_verified};
pub use error::{Error, Result};

/// A caller's namespace and durable operation identity, attached atomically.
#[derive(Clone, Copy)]
pub struct Owner<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

impl Owner<'_> {
    fn validate(self) -> Result<()> {
        if self.key.is_empty()
            || self.key.len() > 128
            || self.value.is_empty()
            || self.value.len() > 256
            || !self
                .key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
            || self.value.chars().any(char::is_control)
        {
            return Err(Error::Uncertain);
        }
        Ok(())
    }
}

/// Start an agent with its ownership label in the atomic core run.
/// The caller durably records its starting intent before calling this.
/// This only starts: the caller journals and sends the prompt separately.
pub fn start(
    host: &Host,
    tree: &Worktree,
    agent: &Agent<'_>,
    owner: Owner<'_>,
    cancelled: &AtomicBool,
) -> Result<Started> {
    owner.validate()?;
    if !matches!(tree.opened, Opened::Herdr2) {
        return Ok(super::start_agent(
            host,
            tree,
            &Agent {
                prompt: None,
                ..*agent
            },
            cancelled,
        )?);
    }
    let argv = command(agent);
    let program = argv
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    // Run the agent itself so a recovered live terminal cannot be a shell
    // into which a different occupant was typed. `keep` never replays it.
    let line = format!(
        "core_cli run --bg --json --no-parent --keep --cwd {} --label {} -- {program}\n",
        shell_quote(&tree.path),
        shell_quote(&format!("{}={}", owner.key, owner.value)),
    );
    let output = host
        .capture(&line, cancelled)
        .map_err(script(Step::StartAgent))?;
    let ran: super::Ran =
        serde_json::from_slice(output.trim_ascii()).map_err(|source| super::Error::Decode {
            step: Step::StartAgent,
            source,
        })?;
    Ok(Started::Herdr2 {
        terminal: ran.address,
    })
}

/// Find exactly the terminal atomically labelled for this lane. CWD, live
/// state and an optional previously saved address fence replaced terminals.
pub fn recover(
    host: &Host,
    tree: &Worktree,
    owner: Owner<'_>,
    agent: &Agent<'_>,
    expected: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Started> {
    inspect(host, tree, owner, agent, expected, false, cancelled)
}

/// Inspect a retained terminal for cleanup. Finished terminals, and live
/// ones whose agent exited and left the kept shell in front, are accepted
/// only when explicitly requested; an unknown state is never ownership proof.
pub fn inspect(
    host: &Host,
    tree: &Worktree,
    owner: Owner<'_>,
    agent: &Agent<'_>,
    expected: Option<&str>,
    include_finished: bool,
    cancelled: &AtomicBool,
) -> Result<Started> {
    owner.validate()?;
    let line = format!(
        "core_cli ls --all --json --label {}\n",
        shell_quote(&format!("{}={}", owner.key, owner.value))
    );
    let output = host.capture(&line, cancelled).map_err(script(Step::List))?;
    let list: List =
        serde_json::from_slice(output.trim_ascii()).map_err(|source| super::Error::Decode {
            step: Step::List,
            source,
        })?;
    let argv = command(agent);
    select(
        list,
        &tree.path,
        owner,
        &argv,
        expected,
        include_finished,
        agent.kind,
    )
}

fn select(
    list: List,
    path: &str,
    owner: Owner<'_>,
    argv: &[String],
    expected: Option<&str>,
    include_finished: bool,
    kind: crate::teleport::AgentKind,
) -> Result<Started> {
    let mut matches = list.terminals.into_iter().filter(|terminal| {
        terminal
            .labels
            .get(owner.key)
            .is_some_and(|value| value == owner.value)
    });
    let terminal = matches.next().ok_or(Error::Uncertain)?;
    if matches.next().is_some()
        || !(terminal.state == TerminalState::Live
            || (include_finished && terminal.state == TerminalState::Finished))
        || terminal.cwd != path
        || terminal.argv != argv
        // Recovery types into the agent, so it must still be in front. A
        // cleanup only ends the terminal, which may also be the shell
        // `--keep` left at its prompt once the agent exited, as every
        // finished run does; a program started there since is the user's.
        || (terminal.state == TerminalState::Live
            && !terminal.foreground.as_ref().is_some_and(|foreground| {
                crate::teleport::AgentKind::identify(None, &foreground.name, &foreground.argv)
                    == Some(kind)
                    || foreground.argv == argv
                    || (include_finished && at_prompt(&foreground.name, &foreground.argv))
            }))
        || expected.is_some_and(|expected| terminal.address != expected)
    {
        return Err(Error::Uncertain);
    }
    Ok(Started::Herdr2 {
        terminal: terminal.address,
    })
}

fn command(agent: &Agent<'_>) -> Vec<String> {
    let mut argv = agent.kind.start_argv(None);
    if let Some(model) = agent.model.filter(|model| super::valid_model(model)) {
        argv.extend(["--model".into(), model.into()]);
    }
    argv
}

/// Whether a foreground process is a bare shell waiting at its prompt:
/// a shell given only options, not a script or command to run.
fn at_prompt(name: &str, argv: &[String]) -> bool {
    match argv.split_first() {
        Some((program, options)) => {
            crate::teleport::is_shell(program)
                && options.iter().all(|arg| arg.starts_with('-'))
                && !options.iter().any(|arg| arg == "-c")
        }
        None => crate::teleport::is_shell(name),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
