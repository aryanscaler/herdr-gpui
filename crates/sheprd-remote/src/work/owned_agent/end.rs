//! Stop only verified resources, and prove that `--keep` did not leave a
//! replacement shell before callers remove the checkout.

use super::{Agent, Error, Host, Owner, Result, Started, Step, Worktree, command, script, select};
use sheprd_client::shell_quote;
use sheprd_core_client::wire::{List, TerminalState};
use std::sync::atomic::AtomicBool;

/// End a directly launched owned agent before deleting its checkout. An
/// unacknowledged launch without a recoverable receipt remains uncertain.
pub fn end(
    host: &Host,
    tree: &Worktree,
    owner: Owner<'_>,
    agent: &Agent<'_>,
    expected: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<()> {
    owner.validate()?;
    let listed = inventory(host, cancelled)?;
    let owned = listed.terminals.iter().any(|terminal| {
        terminal
            .labels
            .get(owner.key)
            .is_some_and(|value| value == owner.value)
    });
    // A different program in this checkout is not ours to close or delete
    // out from under it, even if the owned agent has already disappeared.
    if listed.terminals.iter().any(|terminal| {
        terminal.state != TerminalState::Finished
            && terminal
                .labels
                .get(owner.key)
                .is_none_or(|value| value != owner.value)
            && (inside(&terminal.cwd, &tree.path)
                || terminal
                    .foreground
                    .as_ref()
                    .is_some_and(|foreground| inside(&foreground.cwd, &tree.path)))
    }) {
        return Err(Error::Uncertain);
    }
    if !owned {
        return if expected.is_some_and(|expected| {
            listed
                .terminals
                .iter()
                .all(|terminal| terminal.address != expected)
        }) {
            Ok(())
        } else {
            Err(Error::Uncertain)
        };
    }
    let started = select(
        listed,
        &tree.path,
        owner,
        &command(agent),
        expected,
        true,
        agent.kind,
    )?;
    let Started::Herdr2 { terminal } = started else {
        return Err(Error::Uncertain);
    };
    end_verified(host, &terminal, cancelled)?;
    checkout_idle(host, tree, cancelled)
}

/// Verify that a checkout contains no live core program before removing it,
/// including unrelated shells opened while inspecting an unstarted lane.
pub fn checkout_idle(host: &Host, tree: &Worktree, cancelled: &AtomicBool) -> Result<()> {
    if inventory(host, cancelled)?
        .terminals
        .iter()
        .any(|terminal| {
            terminal.state != TerminalState::Finished
                && (inside(&terminal.cwd, &tree.path)
                    || terminal
                        .foreground
                        .as_ref()
                        .is_some_and(|foreground| inside(&foreground.cwd, &tree.path)))
        })
    {
        return Err(Error::Uncertain);
    }
    Ok(())
}

/// End a terminal whose current ownership the caller has just verified,
/// including an explicitly adopted legacy shell. This is not an atomic
/// ownership-and-kill operation; callers must preserve that limitation.
/// Success means an inventory proved the program gone, not merely a queued
/// kill. The caller must separately verify checkout identity before deletion.
pub fn end_verified(host: &Host, terminal: &str, cancelled: &AtomicBool) -> Result<()> {
    let address = shell_quote(terminal);
    let line = format!(
        "core_cli keep --off --json -- {address} >/dev/null && core_cli kill --json -- {address} >/dev/null\n"
    );
    let outcome = host.capture(&line, cancelled);
    let listed = inventory(host, cancelled)?;
    if let Some(found) = listed
        .terminals
        .iter()
        .find(|found| found.address == terminal)
    {
        if found.state != TerminalState::Finished {
            return match outcome {
                Err(error) => Err(script(Step::Remove)(error).into()),
                Ok(_) => Err(Error::Uncertain),
            };
        }
        host.capture(
            &format!("core_cli rm --json -- {address} >/dev/null\n"),
            cancelled,
        )
        .map_err(script(Step::Remove))?;
    }
    Ok(())
}

fn inventory(host: &Host, cancelled: &AtomicBool) -> Result<List> {
    let output = host
        .capture("core_cli ls --all --json\n", cancelled)
        .map_err(script(Step::List))?;
    Ok(
        serde_json::from_slice(output.trim_ascii()).map_err(|source| {
            crate::work::Error::Decode {
                step: Step::List,
                source,
            }
        })?,
    )
}

fn inside(cwd: &str, checkout: &str) -> bool {
    cwd == checkout
        || cwd
            .strip_prefix(checkout)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
