//! Deliberate adoption of a legacy terminal. The confirmation grants new
//! authority over the displayed current process; it proves no historical link.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const LABEL: &str = "sheprd.orchestrator.adopted";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Process {
    pub address: String,
    pub cwd: String,
    pub argv: Vec<String>,
    pub pid: i32,
    pub foreground: Foreground,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Foreground {
    pub name: String,
    pub argv: Vec<String>,
    pub pid: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub target: ConnectTarget,
    pub process: Process,
    pub token: String,
}

#[derive(Deserialize)]
struct Inventory {
    terminals: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Terminal {
    #[serde(flatten)]
    process: Process,
    state: State,
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum State {
    Live,
    Finished,
    #[serde(other)]
    Other,
}

fn uncertain() -> Error {
    work::owned_agent::Error::Uncertain.into()
}

fn read(host: &Host, address: &str, cancelled: &AtomicBool) -> Result<Terminal> {
    let output = host
        .capture("core_cli ls --all --json\n", cancelled)
        .map_err(script("Reading the agent terminals"))?;
    let list: Inventory = serde_json::from_slice(&output).map_err(|source| Error::OutputJson {
        operation: "Reading the agent terminals",
        source,
    })?;
    let mut matches = list.terminals.into_iter().filter(|terminal| {
        terminal.get("address").and_then(serde_json::Value::as_str) == Some(address)
    });
    let found = matches.next().ok_or_else(uncertain)?;
    if matches.next().is_some() {
        return Err(uncertain());
    }
    serde_json::from_value(found).map_err(|source| Error::OutputJson {
        operation: "Reading the agent terminals",
        source,
    })
}

/// Read only. The caller displays this evidence before offering adoption.
pub fn proposal(
    host: &Host,
    run: &Run,
    session: &HerdrSession,
    cancelled: &AtomicBool,
) -> Result<Receipt> {
    if run.owner != Owner::Sheprd || session.target.is_some() {
        return Err(uncertain());
    }
    let Pane::Herdr2 { terminal } = &session.pane else {
        return Err(uncertain());
    };
    let target = target_of(session);
    let found = read(host, terminal, cancelled)?;
    let expected_path = run
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.path.as_ref())
        .ok_or_else(uncertain)?;
    let process = &found.process;
    let kind = AgentKind::parse(&run.agent).ok_or_else(uncertain)?;
    if found.state != State::Live
        || process.pid <= 0
        || process.foreground.pid <= 0
        || std::path::Path::new(&process.cwd) != expected_path
        || AgentKind::identify(None, &process.foreground.name, &process.foreground.argv)
            != Some(kind)
        || process.argv.len() > 64
        || process.foreground.argv.len() > 64
        || process
            .argv
            .iter()
            .chain(&process.foreground.argv)
            .map(String::len)
            .sum::<usize>()
            > 4096
        || found.labels.iter().any(|(key, value)| {
            key.starts_with("sheprd.orchestrator.")
                && !(key == LABEL
                    && session
                        .adopted
                        .as_ref()
                        .is_some_and(|receipt| &receipt.token == value))
        })
    {
        return Err(uncertain());
    }
    Ok(Receipt {
        target,
        process: found.process,
        token: agent_name(),
    })
}

pub(super) fn adopt(
    site: &Site,
    id: &str,
    expected: &Receipt,
    cancelled: &AtomicBool,
) -> Result<()> {
    let (mut store, run, session) = owned(site, id)?;
    let mut session = session.ok_or_else(uncertain)?;
    let host = host_of(site, &session)?;
    let mut current = proposal(&host, &run, &session, cancelled)?;
    current.token.clone_from(&expected.token);
    if &current != expected {
        return Err(uncertain());
    }
    // Persist the confirmed receipt before the label mutation. A lost label
    // reply never grants authority without its subsequent identity check.
    session.adopted = Some(expected.clone());
    store.update_run(&run, Some(&session))?;
    host.capture(
        &format!(
            "core_cli label --json -- {} {}\n",
            shell_quote(&expected.process.address),
            shell_quote(&format!("{LABEL}={}", expected.token))
        ),
        cancelled,
    )
    .map_err(script("Adopting the agent terminal"))?;
    verify(&host, expected, false, cancelled).map(drop)
}

pub(super) fn verify(
    host: &Host,
    receipt: &Receipt,
    cleanup: bool,
    cancelled: &AtomicBool,
) -> Result<work::Started> {
    let found = read(host, &receipt.process.address, cancelled)?;
    let same = found.process == receipt.process;
    let shell_after_stop = cleanup
        && found.process.address == receipt.process.address
        && found.process.pid == receipt.process.pid
        && found.process.argv == receipt.process.argv
        && found.process.cwd == receipt.process.cwd
        && found.process.foreground.pid == found.process.pid;
    if found.state != State::Live
        || (!same && !shell_after_stop)
        || found.labels.get(LABEL) != Some(&receipt.token)
    {
        return Err(uncertain());
    }
    Ok(work::Started::Herdr2 {
        terminal: found.process.address,
    })
}
