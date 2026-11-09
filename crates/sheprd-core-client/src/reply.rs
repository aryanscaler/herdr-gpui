//! Typing a reply to an agent in a herdr-core terminal. `terminal.send`
//! types into whatever program has the terminal, a shell included, so the
//! reply is checked against what the core says now, just before it goes,
//! and an idle agent's is typed only while the core still reports it idle.

use crate::{
    Core, Result, agent_name,
    backend::info,
    wire::{SendText, StatusState, Terminal},
};
use sheprd_client::herdr::AgentKind;

/// What became of a reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replied {
    Sent,
    /// The agent it was written to no longer runs in the terminal.
    Gone,
}

/// Blocking: types `text` into `terminal` for the agent named `agent`, if
/// that agent still has it. The core refuses a blocked agent
/// ([`crate::wire::BLOCKED`]), and an idle one that went to work meanwhile
/// ([`crate::wire::STATUS_MISMATCH`]).
pub fn reply(core: &Core, terminal: &str, agent: &str, text: &str) -> Result<Replied> {
    let listed = core.list(false)?;
    let Some(record) = listed
        .terminals
        .iter()
        .find(|record| record.address == terminal)
    else {
        return Ok(Replied::Gone);
    };
    if !agent_in_front(record, agent) {
        return Ok(Replied::Gone);
    }
    let idle = record
        .status
        .as_ref()
        .is_some_and(|status| status.state == StatusState::Idle);
    core.send(&SendText {
        terminal,
        text,
        only_if: idle.then_some("idle"),
        ..SendText::default()
    })?;
    Ok(Replied::Sent)
}

/// Whether `record`'s terminal still runs the agent named `agent`. Its
/// report outlives it, so the terminal's own shell back in front means it
/// left, unless the agent is the program the terminal started.
pub fn agent_in_front(record: &Terminal, agent: &str) -> bool {
    let Ok(now) = info(record) else {
        return false;
    };
    if agent_name(&now).as_deref() != Some(agent) {
        return false;
    }
    let own_program_in_front = record
        .pid
        .zip(record.foreground.as_ref())
        .is_some_and(|(pid, front)| front.pid == pid);
    let program = record.argv.first().map_or("", String::as_str);
    !own_program_in_front || AgentKind::identify(None, program, &record.argv).is_some()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
