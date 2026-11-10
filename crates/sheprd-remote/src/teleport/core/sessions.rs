//! Agent sessions in Herdr 2 terminals. herdr-core reports which agent runs
//! in a terminal but not its session, which Herdr 1 learns from hooks of its
//! own. Claude Code records its session by process in
//! `~/.claude/sessions/<pid>.json`, so a Claude Code terminal's session is
//! read from there: the record of the terminal's foreground process, started
//! when that process was, so a record left by an earlier process with the
//! same ID is never taken for it. Other agents keep no such record, and
//! hand off with a note as before.

use super::Terminal;
use crate::teleport::{
    error::{Error, Result, Step},
    host::Host,
    launch::{AgentKind, Work},
    snapshot::{AgentSession, SessionKind},
};
use serde::Deserialize;
use std::{collections::HashMap, sync::atomic::AtomicBool};

/// The most of one record read; Claude Code's are well under a kilobyte.
const MAX_RECORD: usize = 64 * 1024;

/// What Claude Code records of one running process, reduced to what ties it
/// to the process and names its session.
#[derive(Debug, Deserialize)]
struct Record {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    /// When the process started, as `ps -o lstart` prints it in UTC.
    #[serde(rename = "procStart", default)]
    proc_start: Option<String>,
}

/// The terminals of `terminals` that run Claude Code, by their foreground
/// process.
fn claude_processes(terminals: &HashMap<String, Terminal>) -> Vec<(&str, u32)> {
    let mut found: Vec<(&str, u32)> = terminals
        .iter()
        .filter_map(|(id, terminal)| {
            let Work::Agent { name, .. } = terminal.work() else {
                return None;
            };
            let pid = terminal.foreground.as_ref()?.pid?;
            (AgentKind::parse(&name) == Some(AgentKind::Claude)).then_some((id.as_str(), pid))
        })
        .collect();
    found.sort_unstable();
    found
}

/// The Claude Code session of each terminal of `terminals` that runs one
/// and recorded it. A host that cannot say leaves the agents to hand off,
/// so a failure here never fails the move.
pub(in crate::teleport) fn sessions(
    host: &Host,
    terminals: &HashMap<String, Terminal>,
    cancelled: &AtomicBool,
) -> Result<HashMap<String, AgentSession>> {
    let processes = claude_processes(terminals);
    if processes.is_empty() {
        return Ok(HashMap::new());
    }
    let body = processes
        .iter()
        .map(|(id, pid)| {
            // The ID and both start times, then the record, if any.
            format!(
                r#"printf '\036%s\n' {id}
f="${{CLAUDE_CONFIG_DIR:-$HOME/.claude}}/sessions/{pid}.json"
if [ -f "$f" ]; then
printf '%s\n' "$(LC_ALL=C TZ=UTC ps -o lstart= -p {pid} 2>/dev/null)"
printf '%s\n' "$(LC_ALL=C ps -o lstart= -p {pid} 2>/dev/null)"
head -c {MAX_RECORD} "$f"
fi
"#,
                id = sheprd_client::shell_quote(id),
            )
        })
        .collect::<String>();
    let output = host.query(Step::Review, &body, &[], cancelled)?;
    let output = String::from_utf8_lossy(&output);
    Ok(parse(&output, &processes))
}

/// Each section of `output`: a terminal's ID, its process's start in UTC
/// and in local time, then Claude Code's record of the process.
fn parse(output: &str, processes: &[(&str, u32)]) -> HashMap<String, AgentSession> {
    let mut found = HashMap::new();
    for section in output.split('\u{1e}').filter(|section| !section.is_empty()) {
        let mut lines = section.splitn(4, '\n');
        let (Some(id), Some(utc), Some(local), Some(json)) =
            (lines.next(), lines.next(), lines.next(), lines.next())
        else {
            continue;
        };
        let Some((_, pid)) = processes.iter().find(|(known, _)| *known == id) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<Record>(json.trim()) else {
            continue;
        };
        let started = record.proc_start.as_deref().map(words);
        let same_process = record.pid == *pid
            && started.is_some_and(|started| {
                !started.is_empty() && (started == words(utc) || started == words(local))
            });
        let session = AgentSession {
            agent: AgentKind::Claude.name().to_owned(),
            kind: SessionKind::Id,
            source: format!("herdr:{}", AgentKind::Claude.name()),
            value: record.session_id,
        };
        if same_process && AgentKind::Claude.accepts(&session) {
            found.insert(id.to_owned(), session);
        }
    }
    found
}

/// `text` with its runs of spaces made one, as `ps` pads its columns.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Refuses a move whose Herdr 2 agents no longer run the sessions it
/// reviewed, `expected` by terminal: one resumed or restarted since would
/// have its old conversation resumed on the destination. Asked before
/// anything changes.
pub(in crate::teleport) fn confirm(
    host: &Host,
    expected: &[(String, AgentSession)],
    cancelled: &AtomicBool,
) -> Result<()> {
    if expected.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = expected.iter().map(|(id, _)| id.clone()).collect();
    let terminals = super::terminals(host, &ids, cancelled)?;
    let now = sessions(host, &terminals, cancelled)?;
    unchanged(expected, &now)
}

fn unchanged(
    expected: &[(String, AgentSession)],
    now: &HashMap<String, AgentSession>,
) -> Result<()> {
    if expected
        .iter()
        .all(|(id, session)| now.get(id) == Some(session))
    {
        Ok(())
    } else {
        Err(Error::SourceSessionChanged)
    }
}

/// What `terminal` runs, resumable when it is an agent whose `session` was
/// found.
pub(in crate::teleport) fn work(terminal: &Terminal, session: Option<&AgentSession>) -> Work {
    let work = terminal.work();
    let (Work::Agent { name, argv }, Some(session)) = (&work, session) else {
        return work;
    };
    match AgentKind::parse(name) {
        Some(agent) if agent.carries() && agent.accepts(session) => Work::Session {
            agent,
            session: session.clone(),
            flags: agent.kept_flags(argv),
        },
        _ => work,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "tests/sessions.rs"]
mod tests;
