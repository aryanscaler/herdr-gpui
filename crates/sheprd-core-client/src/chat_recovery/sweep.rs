use super::*;
use crate::wire::TerminalState;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

/// One bounded pass per few seconds on an existing connection worker, never
/// a UI thread. The cursor lets every chat be visited when a host has many.
#[derive(Default)]
pub struct Sweep {
    after: Option<String>,
    next: Option<Instant>,
}

impl Sweep {
    pub fn poll(&mut self, core: &Core) {
        let now = Instant::now();
        if self.next.is_some_and(|next| now < next) {
            return;
        }
        self.next = Some(now + Duration::from_secs(3));
        if let Err(error) = self.pass(core) {
            tracing::debug!(%error, "chat recovery pass stopped");
        }
    }

    fn pass(&mut self, core: &Core) -> Result<()> {
        let terminals = core.list(true)?.terminals;
        let ids: BTreeSet<_> = terminals
            .iter()
            .take(4096)
            .filter(|terminal| !terminal.address.contains('/'))
            .filter_map(|terminal| {
                terminal
                    .labels
                    .get(TOKEN)
                    .or_else(|| terminal.labels.get(PENDING_LABEL))
            })
            .filter(|id| id.parse::<EndpointId>().is_ok())
            .cloned()
            .collect();
        let start = self
            .after
            .as_ref()
            .map_or(0, |after| ids.iter().take_while(|id| *id <= after).count());
        let count = ids.len().min(16);
        for id in ids.iter().cycle().skip(start).take(count) {
            self.after = Some(id.clone());
            let endpoint = id.parse().map_err(|_| Error::Invalid)?;
            if let Err(error) = recover(core, &endpoint, &terminals) {
                tracing::debug!(%error, "chat recovery deferred");
            }
        }
        Ok(())
    }
}

fn recover(core: &Core, endpoint: &EndpointId, terminals: &[Terminal]) -> Result<()> {
    let (record, version) = read(core, endpoint)?;
    let Some(mut record) = record else {
        return Ok(());
    };
    if record.state == State::Closed {
        if !record.previous.is_empty()
            && terminals
                .iter()
                .any(|terminal| terminal.address == record.terminal)
        {
            // Keep the durable forward receipt even when the replacement
            // ended before a client saved its rewritten layout. Best effort:
            // a removed retired shell must never keep the kills below from
            // ending a replacement the close outlived.
            if let Err(error) = publish_mapping(core, &record) {
                tracing::debug!(%error, "closed chat receipts deferred");
            }
        }
        // A close can race a run already in flight. Its ACP startup guard
        // refuses the tombstone; also end any replacement that claimed first.
        // Retired recovery shells are never killed: a user may be using one.
        for terminal in terminals.iter().filter(|terminal| {
            terminal.state == TerminalState::Live
                && terminal
                    .labels
                    .get(TOKEN)
                    .or_else(|| terminal.labels.get(PENDING_LABEL))
                    .map(String::as_str)
                    == Some(endpoint.as_str())
                && (chat_program(terminal)
                    || terminal
                        .argv
                        .first()
                        .is_some_and(|program| *program == record.program))
        }) {
            core.kill(&terminal.address)?;
        }
        return Ok(());
    }
    let original = terminals
        .iter()
        .find(|terminal| terminal.address == record.terminal);
    if original.is_none_or(|terminal| terminal.state == TerminalState::Finished) {
        // Missing is not evidence of restart: it could be an explicit rm.
        return close_if_ended(core, endpoint, record, version);
    }
    let Some(original) = original else {
        return Ok(());
    };
    if record.state == State::Starting {
        return Ok(());
    }
    if record.state == State::Live {
        if !lost(&record, original) {
            if receipts_missing(&record, original, terminals) {
                publish_mapping(core, &record)?;
            }
            return Ok(());
        }
        if record.previous.len() >= MAX_PREVIOUS {
            return Err(Error::Limit.into());
        }
        record.previous.push(record.terminal.clone());
        record.generation = record.generation.checked_add(1).ok_or(Error::Limit)?;
        record.state = State::Launching;
        // CAS claims before launch, durable across app/phone worker crashes.
        write(core, endpoint, &record, version)?;
    }
    let name = launch_name(endpoint, record.generation);
    if terminals
        .iter()
        .any(|terminal| terminal.name.as_deref() == Some(&name))
    {
        // Includes finished attempts. A lost run reply never causes us to
        // replay a launch that started and failed before we could observe it.
        return Ok(());
    }
    // Re-read the claim immediately before launch. The ACP startup guard is
    // the final fence if an explicit close races this request or its reply.
    let (current, _) = read(core, endpoint)?;
    if current
        .as_ref()
        .is_none_or(|now| now.state != State::Launching || now.generation != record.generation)
    {
        return Err(Error::Moved.into());
    }
    let mut argv = vec![
        record.program.clone(),
        "--resume".to_owned(),
        record.session.clone(),
    ];
    if record.adapter {
        argv.push("--acp".to_owned());
    }
    argv.push(record.agent.clone());
    if !record.args.is_empty() {
        argv.push("--".to_owned());
        argv.extend(record.args.iter().cloned());
    }
    let generation = record.generation.to_string();
    let overrides = serde_json::to_string(&record.env)?;
    let mut env: BTreeMap<&str, &str> = record
        .env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    env.insert(ENDPOINT_ENV, endpoint.as_str());
    env.insert("HERDR_PANE_ID", "");
    env.insert(GENERATION_ENV, &generation);
    env.insert(OVERRIDES_ENV, &overrides);
    // It is not an attachable chat or adoptable workspace pane until ACP has
    // proved the old child gone and claimed this generation on the host.
    let labels = BTreeMap::from([(PENDING_LABEL, endpoint.as_str())]);
    core.run(&Run {
        argv: &argv,
        cwd: Some(&record.cwd),
        env,
        labels,
        keep: true,
        name: Some(&name),
        ..Run::default()
    })?;
    Ok(())
}

/// Tombstones a chat whose terminal is gone or finished, but only on proof
/// newer than the record. `terminals` may predate the record: another worker
/// can create and claim a replacement after this pass listed, so a terminal
/// the record names may simply be too new for that list. A list taken after
/// reading `version`, and a write fenced on it, close only what really ended.
fn close_if_ended(
    core: &Core,
    endpoint: &EndpointId,
    mut record: Record,
    version: u64,
) -> Result<()> {
    let terminals = core.list(true)?.terminals;
    if terminals.iter().any(|terminal| {
        terminal.address == record.terminal && terminal.state != TerminalState::Finished
    }) {
        return Err(Error::Moved.into());
    }
    record.state = State::Closed;
    write(core, endpoint, &record, version)
}

/// Whether either receipt of the last replacement is missing: the
/// replacement's `replaces`, or the retired shell's `replaced_by`, which
/// keeps readers from adopting that shell again as a workspace tab. A
/// retired shell that is gone needs no receipt.
fn receipts_missing(record: &Record, replacement: &Terminal, terminals: &[Terminal]) -> bool {
    let Some(previous) = record.previous.last() else {
        return false;
    };
    replaces(&replacement.labels) != record.previous
        || terminals
            .iter()
            .find(|terminal| terminal.address == *previous)
            .is_some_and(|retired| retired.labels.get(REPLACED_BY_LABEL) != Some(&record.terminal))
}

fn lost(record: &Record, terminal: &Terminal) -> bool {
    if terminal.state != TerminalState::Live || !terminal.keep || chat_program(terminal) {
        return false;
    }
    let Some(front) = terminal.foreground.as_ref() else {
        return false;
    };
    let shell = front.name.trim_end_matches(".exe");
    // No input is sent to this shell; waiting for a replacement root to be
    // observed ensures core has completed its old process cleanup first.
    terminal.pid == Some(front.pid)
        && matches!(
            shell,
            "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" | "nu" | "pwsh" | "powershell" | "cmd"
        )
        && (terminal.pid != record.root_pid || terminal.argv != record.root_argv)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
