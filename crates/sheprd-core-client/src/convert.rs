//! Moving an agent in a herdr-core terminal into a chat: the policy of
//! `sheprd_client::herdr::convert`, asked of herdr-core instead of Herdr 1.
//! The app and the remote control daemon share it.
//!
//! Only an idle agent moves, and everything that could refuse is checked
//! before it is touched. It stops on Ctrl+C (`terminal.keys`) until the shell
//! that started it is back in front, and the `sheprd-acp --resume` line is
//! then typed into that shell (`terminal.send`). The line names the chat's
//! endpoint, and the terminal is labelled with it (`terminal.label`), as a
//! chat started in a terminal of its own is, so every client finds it.
//!
//! Separately installed Sheprd hooks record process-bound session evidence in
//! the core's document store. Claude's own PID file is a legacy fallback,
//! checked against its process birth stamp. No newest-session inference occurs.
//! Client rereads are not an atomic process precondition on keys/send: the core
//! protocol currently provides no such mutation fence.

use crate::{Core, Result, agent_kind, backend::info, wire::Terminal};
use sheprd_backend::{AgentStatus, Process, TerminalId, TerminalInfo};
use sheprd_chat_protocol::{Agent, ENDPOINT_ENV, EndpointId, TOKEN, restore};
pub use sheprd_client::herdr::convert::claude_record;
use sheprd_client::herdr::{
    AgentKind,
    convert::{self, Resume},
};
use std::{path::Path, sync::atomic::AtomicBool};

mod flags;

/// The names a chat program runs under, as the OS reports them.
const CHAT_PROGRAMS: &[&str] = &["herdr-acp", "sheprd-acp"];

/// Herdr 1's pane variable, cleared so a chat in a herdr-core terminal never
/// reports to a Herdr 1 pane the core happened to inherit.
const PANE_ENV: &str = "HERDR_PANE_ID";

/// The chat endpoint `info` runs: its token label, unless herdr-core reports
/// another program than a chat in front. A terminal an agent moved out of
/// keeps the label once its chat ends and its shell is back in front.
pub fn chat_endpoint(info: &TerminalInfo) -> Option<EndpointId> {
    let endpoint = info.labels.get(TOKEN)?.parse().ok()?;
    let chat = info.foreground.as_ref().is_some_and(|process| {
        let name = process.name.strip_suffix(".exe").unwrap_or(&process.name);
        CHAT_PROGRAMS.contains(&name)
    });
    chat.then_some(endpoint)
}

/// Whether the terminal's agent is one that can move at all, however busy
/// it is.
pub fn movable(info: &TerminalInfo) -> Result<Agent, convert::Error> {
    if chat_endpoint(info).is_some() {
        return Err(convert::Error::AlreadyChat);
    }
    let kind = agent_kind(info).ok_or(convert::Error::NoAgent)?;
    let agent: Agent = kind
        .name()
        .parse()
        .map_err(|_| convert::Error::Unknown(kind.name().to_owned()))?;
    if !convert::convertible(agent) {
        return Err(convert::Error::Unsupported(agent));
    }
    Ok(agent)
}

/// Whether the terminal's agent may move now: movable, and between turns.
pub fn check(info: &TerminalInfo) -> Result<Agent, convert::Error> {
    let agent = movable(info)?;
    match info.agent {
        AgentStatus::Idle | AgentStatus::Done => Ok(agent),
        AgentStatus::Working | AgentStatus::Blocked | AgentStatus::Unknown => {
            Err(convert::Error::Busy(agent))
        }
    }
}

/// The process in front when it runs `agent`.
fn agent_process(info: &TerminalInfo, agent: Agent) -> Result<&Process, convert::Error> {
    info.foreground
        .as_ref()
        .filter(|process| runs(process, agent))
        .ok_or(convert::Error::NotRunning(agent))
}

/// Whether `process` is `agent` itself, not a shell or another program.
fn runs(process: &Process, agent: Agent) -> bool {
    AgentKind::identify(None, &process.name, &process.argv)
        .is_some_and(|kind| kind.name() == agent.name())
}

/// Whether the shell the terminal started has it again: the process in
/// front is the terminal's own program. Any other program in front, such as
/// one the agent's command line runs after it (`claude; less log`), must not
/// get the chat's line.
pub fn shell_back(record: &Terminal, agent: Agent) -> bool {
    record
        .pid
        .zip(record.foreground.as_ref())
        .is_some_and(|(pid, front)| front.pid == pid && !front_runs(front, agent))
}

/// Whether the process in front is `agent`.
fn front_runs(front: &crate::wire::Foreground, agent: Agent) -> bool {
    AgentKind::identify(None, &front.name, &front.argv)
        .is_some_and(|kind| kind.name() == agent.name())
}

/// Whether `agent` is the program the terminal started, so stopping it
/// ends the terminal rather than handing it back to a shell.
fn agent_is_program(record: &Terminal, agent: Agent) -> bool {
    record
        .pid
        .zip(record.foreground.as_ref())
        .is_some_and(|(pid, front)| front.pid == pid && front_runs(front, agent))
}

/// The line that starts `program` on `resume`'s session at `endpoint` in a
/// Unix shell, through `env`, so the variables reach it whatever the shell.
pub fn chat_line(
    resume: &Resume,
    program: &Path,
    endpoint: &EndpointId,
) -> Result<String, convert::Error> {
    let program = program.to_str().ok_or(convert::Error::Line)?;
    let endpoint = format!("{ENDPOINT_ENV}={endpoint}");
    let pane = format!("{PANE_ENV}=");
    let mut words = vec!["env", pane.as_str(), endpoint.as_str(), program];
    words.extend(resume.words());
    sheprd_client::quoted_line(&words).ok_or(convert::Error::Line)
}

/// What the core reports of `terminal` now, as it sends it.
fn record(core: &Core, terminal: &TerminalId) -> Result<Terminal> {
    core.list(false)?
        .terminals
        .into_iter()
        .find(|record| record.address == terminal.as_str())
        .ok_or_else(|| convert::Error::Moved.into())
}

/// Blocking: moves the agent in `terminal`, which a client last saw as
/// `agent`, into a chat run by `program`, a path on this machine. `home`
/// holds the agent's own records. Refusals before the agent stops leave it
/// running; `cancelled` ends the move between rounds. Returns the chat's
/// endpoint, which the terminal is labelled with.
pub fn run(
    core: &Core,
    terminal: &TerminalId,
    agent: Agent,
    program: &Path,
    home: &Path,
    cancelled: &AtomicBool,
) -> Result<EndpointId> {
    if cfg!(windows) {
        // The shared launch line targets a POSIX shell. The CLI helper must
        // enforce the same platform boundary as desktop and phone callers.
        return Err(convert::Error::Unsupported(agent).into());
    }
    // This entry point reads local process records and starts a local binary.
    // A routed address must never borrow a same-numbered local process.
    if terminal.as_str().contains('/') {
        return Err(convert::Error::Moved.into());
    }
    let raw = record(core, terminal)?;
    let found = info(&raw)?;
    if check(&found)? != agent {
        return Err(convert::Error::Moved.into());
    }
    // Refused before it is touched: stopping it would end the terminal.
    if agent_is_program(&raw, agent) {
        return Err(convert::Error::NoShell(agent).into());
    }
    let kind = AgentKind::parse(agent.name()).ok_or(convert::Error::NoAgent)?;
    let binding = match crate::sessions::discover_in(core, terminal.as_str(), kind, Some(home)) {
        Ok(binding) => binding,
        Err(crate::Error::Sessions(
            crate::sessions::Error::Absent | crate::sessions::Error::Cleared,
        )) => {
            return Err(convert::Error::NoCoreSession(agent).into());
        }
        Err(error) => return Err(error),
    };
    let resume = {
        let process = agent_process(&found, agent)?;
        let selected = binding
            .selected()
            .filter(|session| {
                session.kind == sheprd_client::herdr::snapshot::SessionKind::Id
                    && restore::session_id_is_plain(&session.value)
            })
            .ok_or(convert::Error::NoCoreSession(agent))?;
        Resume {
            agent,
            cwd: found
                .cwd
                .as_deref()
                .and_then(Path::to_str)
                .filter(|cwd| !cwd.is_empty())
                .ok_or(convert::Error::NotRunning(agent))?
                .to_owned(),
            flags: flags::chat(kind, &process.argv)?,
            session: selected.value.clone(),
        }
    };
    resume.folder_here()?;
    let endpoint = EndpointId::random();
    let line = bound_line(&resume, program, &endpoint, &binding.environment)?;
    // Read again after constructing the line; selection can change without
    // changing the foreground PID (notably OpenCode's session tabs).
    if crate::sessions::discover_in(core, terminal.as_str(), kind, Some(home))? != binding {
        return Err(convert::Error::Moved.into());
    }
    convert::stop(
        agent,
        || record(core, terminal).map(|now| returned(&raw, &now, agent)),
        || {
            let now = record(core, terminal)?;
            unchanged(&raw, &now, agent)?;
            if crate::sessions::discover_in(core, terminal.as_str(), kind, Some(home))? != binding {
                return Err(convert::Error::Moved.into());
            }
            core.keys(terminal.as_str(), &["ctrl+c"])
        },
        || std::thread::sleep(convert::POLL),
        cancelled,
    )?;
    // Stopped once the agent left, as when chats were turned off: its
    // session is left to continue by hand, and no chat starts.
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(convert::Error::Stopped.into());
    }
    if !returned(&raw, &record(core, terminal)?, agent) {
        return Err(convert::Error::Moved.into());
    }
    core.send(&crate::wire::SendText {
        terminal: terminal.as_str(),
        text: &line,
        ..Default::default()
    })?;
    core.label(
        terminal.as_str(),
        &std::collections::BTreeMap::from([(TOKEN, endpoint.as_str())]),
    )?;
    Ok(endpoint)
}

fn bound_line(
    resume: &Resume,
    program: &Path,
    endpoint: &EndpointId,
    environment: &std::collections::BTreeMap<String, String>,
) -> Result<String, convert::Error> {
    let mut words = vec![
        "env".to_owned(),
        format!("{PANE_ENV}="),
        format!("{ENDPOINT_ENV}={endpoint}"),
    ];
    words.extend(
        environment
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    words.push(program.to_str().ok_or(convert::Error::Line)?.to_owned());
    words.extend(resume.words().into_iter().map(str::to_owned));
    sheprd_client::quoted_line(&words.iter().map(String::as_str).collect::<Vec<_>>())
        .ok_or(convert::Error::Line)
}

/// A replacement terminal's shell is not the shell that launched this agent.
fn returned(before: &Terminal, now: &Terminal, agent: Agent) -> bool {
    before.pid == now.pid && before.pid.is_some() && shell_back(now, agent)
}

/// Recheck identity and idleness after discovery and before every interrupt.
/// The original foreground, not merely another process of the same agent kind,
/// owns the conversation being moved.
fn unchanged(before: &Terminal, now: &Terminal, agent: Agent) -> Result<()> {
    if before.pid != now.pid || before.foreground != now.foreground {
        return Err(convert::Error::Moved.into());
    }
    if check(&info(now)?)? != agent {
        return Err(convert::Error::Moved.into());
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
