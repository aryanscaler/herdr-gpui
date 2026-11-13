//! Agent sessions in Herdr 2 terminals, discovered on the owning host through
//! the shared session reporter. Claude continues using its native PID/start
//! record, without requiring a separately installed reporter.

use super::Terminal;
use crate::teleport::{
    error::{Error, Result, Step},
    host::Host,
    launch::{AgentKind, Work},
    snapshot::{AgentSession, SessionKind},
};
use serde::Deserialize;
use sheprd_client::herdr::sessions::Binding;
use std::{collections::HashMap, sync::atomic::AtomicBool};

mod reported;

#[derive(Default)]
pub(in crate::teleport) struct Discovered {
    pub(in crate::teleport) sessions: HashMap<String, AgentSession>,
    pub(in crate::teleport) bindings: HashMap<String, Binding>,
}

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

/// Exact sessions reported on the owning host. Explicit absence permits
/// handoff; failed or stale evidence refuses the move. Only helper-free hosts
/// use the legacy Claude record probe below.
pub(in crate::teleport) fn sessions(
    host: &Host,
    terminals: &HashMap<String, Terminal>,
    cancelled: &AtomicBool,
) -> Result<Discovered> {
    let (mut found, unavailable) = reported::discover(host, terminals, cancelled)?;
    let processes = claude_processes(terminals)
        .into_iter()
        .filter(|(id, _)| unavailable.iter().any(|missing| missing == id))
        .collect::<Vec<_>>();
    if processes.is_empty() {
        return Ok(found);
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
    found.sessions.extend(parse(&output, &processes));
    Ok(found)
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
        if same_process
            && let Ok(session) = sheprd_client::herdr::sessions::reference(
                AgentKind::Claude,
                SessionKind::Id,
                record.session_id,
            )
        {
            found.insert(id.to_owned(), session);
        }
    }
    found
}

/// `text` with its runs of spaces made one, as `ps` pads its columns.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
