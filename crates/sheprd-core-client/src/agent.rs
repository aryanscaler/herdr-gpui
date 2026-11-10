//! A coding agent started with its first prompt, as `hc`'s manual asks:
//! start it, wait until it says it is ready, then send the prompt, since a
//! prompt typed into an agent still booting can be dropped. The same steps
//! serve every agent herdr-core can tell is working, whoever starts it.

use crate::{
    Core, Error, Result,
    connection::CALL_TIMEOUT,
    wire::{Run, SendText, Sent, Terminal, WaitReason, Waited},
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// How long a new agent has to say it is ready, as Herdr 1's `agent start`
/// allows.
pub const READY_TIMEOUT: Duration = Duration::from_secs(90);

/// The longest one wait call is asked to take, so it answers well within a
/// call's own limit and a stop request is seen soon.
const CHECK_IN: Duration = Duration::from_secs(5);

/// How long to pause before asking again about an agent that is not
/// working, which the core answers at once.
const QUIET_PAUSE: Duration = Duration::from_secs(1);

/// When waiting gives up: a stop request, or a deadline.
#[derive(Clone, Copy)]
pub struct Until<'a> {
    pub deadline: Instant,
    pub stop: &'a AtomicBool,
}

impl Until<'_> {
    /// Whether to go on: a stop request is `Error::Stopped`, a passed
    /// deadline `Error::Expired`. Checked before work that cannot be undone,
    /// such as starting an agent.
    pub fn check(&self) -> Result<()> {
        if self.stop.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        if Instant::now() >= self.deadline {
            return Err(Error::Expired);
        }
        Ok(())
    }

    /// How long the next wait call may take, or `None` once the deadline
    /// has passed.
    pub(crate) fn check_in(&self) -> Result<Option<Duration>> {
        if self.stop.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        let left = self.deadline.saturating_duration_since(Instant::now());
        Ok((!left.is_zero()).then(|| left.min(CHECK_IN).min(CALL_TIMEOUT / 2)))
    }
}

/// Starts `run`, an agent, and once it is ready sends it `prompt`. The
/// terminal stays when the prompt fails, so the agent can be read and
/// told again; the error says which step failed.
pub fn launch(core: &Core, run: &Run<'_>, prompt: &str, until: Until<'_>) -> Result<Started> {
    let terminal = core.run(run)?;
    let sent = self::prompt(core, &terminal.address, prompt, until)?;
    Ok(Started { terminal, sent })
}

/// Sends `text` to the agent in `terminal` once it is [ready]: at most
/// [`READY_TIMEOUT`] after this is called, or at the stop or deadline.
pub fn prompt(core: &Core, terminal: &str, text: &str, until: Until<'_>) -> Result<Sent> {
    let ready_by = Until {
        deadline: until.deadline.min(Instant::now() + READY_TIMEOUT),
        ..until
    };
    ready(core, terminal, ready_by)?;
    core.send(&SendText {
        terminal,
        text,
        ..SendText::default()
    })
}

/// A started agent and how its prompt went in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub sent: Sent,
    pub terminal: Terminal,
}

/// Waits until `terminal`'s agent says it is ready for a prompt. Typing
/// into one that has not said so could answer a dialog it shows, such as
/// whether to trust its folder, so silence by the deadline, or a dialog, is
/// an error that leaves the agent for the user to answer.
pub fn ready(core: &Core, terminal: &str, until: Until<'_>) -> Result<()> {
    while let Some(timeout) = until.check_in()? {
        if let Some(ready) = readiness(&core.wait(terminal, timeout)?) {
            return ready;
        }
    }
    Err(Error::AgentNotReady { status: None })
}

/// What one wait on a new agent says about its readiness: `None` while it
/// is still starting, so the caller waits again, else whether it is ready
/// for its prompt. Shared with callers that wait through `herdr-core wait
/// --json`, which answers the same record.
pub fn readiness(waited: &Waited) -> Option<Result<()>> {
    if !waited.ended {
        return None;
    }
    if let Err(error) = ended(waited) {
        return Some(Err(error));
    }
    Some(match waited.status.as_deref() {
        Some("blocked" | "error") | None => Err(Error::AgentNotReady {
            status: waited.status.clone(),
        }),
        Some(_) => Ok(()),
    })
}

/// Waits until the turn after `turn` ends: the agent took a prompt sent at
/// `turn` and is done with it. An agent blocked on a question has not
/// finished, so the wait goes on for the user to answer it; one that
/// reports an error has failed. A deadline passing first is
/// `Error::AgentBusy`.
pub fn finish(core: &Core, terminal: &str, turn: u64, until: Until<'_>) -> Result<Waited> {
    while let Some(timeout) = until.check_in()? {
        let waited = core.wait(terminal, timeout)?;
        if !waited.ended {
            continue;
        }
        ended(&waited)?;
        let finished = waited.turn > turn || waited.reason != WaitReason::Status;
        match waited.status.as_deref() {
            Some("error") if finished => {
                return Err(Error::AgentFailed {
                    status: "error".to_owned(),
                });
            }
            Some("blocked") => {}
            _ if finished => return Ok(waited),
            _ => {}
        }
        // Not started on the prompt yet, or waiting on the user: the core
        // answers such an agent at once, so pause rather than ask again.
        std::thread::sleep(QUIET_PAUSE);
    }
    Err(Error::AgentBusy)
}

/// A wait that ended because the program did is the agent gone.
fn ended(waited: &Waited) -> Result<()> {
    match waited.reason {
        WaitReason::Exit => Err(Error::AgentExited {
            code: waited.exit_code,
        }),
        _ => Ok(()),
    }
}
