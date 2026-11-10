//! Herdr 2 (herdr-core) terminals in a teleported workspace. herdr-core keeps
//! no tabs, the app does, so the app hands over each tab's pane tree; what
//! runs in each terminal is read through the host's `herdr-core` CLI, as the
//! `herdr` CLI reads a Herdr 1 pane. On the destination, the same CLI starts
//! a shell per pane, and the app opens the tabs it returns.

use super::{
    error::{Result, Step, decode},
    host::Host,
    launch::{AgentKind, Work},
    layout::Node,
    snapshot::SplitDirection,
};
use serde::Deserialize;
use sheprd_backend::TerminalId;
use sheprd_client::ScriptLimits;
use sheprd_client::shell_quote;
use sheprd_session::panes::{Axis, Node as PaneNode, Panes, Share};
use std::{collections::HashMap, sync::atomic::AtomicBool, time::Duration};

mod sessions;
pub(super) use sessions::{confirm, sessions, work};

/// How long past a handoff's own timeout its script may stay quiet.
const PROMPT_GRACE: Duration = Duration::from_secs(30);

/// The exit status `herdr-core` gives a terminal it does not know, which a
/// stop treats as already gone.
const NOT_FOUND: u8 = 3;

/// Shells a terminal starts with when it was opened with no program; one in
/// the foreground is an idle prompt.
const SHELLS: &[&str] = &[
    // keep-sorted start
    "bash", "csh", "dash", "elvish", "fish", "ksh", "nu", "pwsh", "sh", "tcsh", "xonsh",
    "zsh",
    // keep-sorted end
];

/// One terminal as `herdr-core get --json` reports it, reduced to what a
/// move reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Terminal {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground: Option<Foreground>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub status: Option<Status>,
}

/// The terminal's foreground process: its shell at an idle prompt.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Foreground {
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
}

/// What herdr-core recognized running, such as `claude-code`.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Status {
    #[serde(default)]
    pub app: Option<String>,
}

impl Terminal {
    /// Where its program works: the foreground process's folder, else the
    /// terminal's.
    pub(super) fn cwd(&self) -> Option<String> {
        self.foreground
            .as_ref()
            .and_then(|foreground| foreground.cwd.clone())
            .or_else(|| self.cwd.clone())
    }

    /// What the terminal runs, as a Herdr 1 pane's process info would say.
    /// herdr-core reports no agent session, so an agent is one to start
    /// afresh with a handoff note, unless [`work`] finds its session.
    pub(super) fn work(&self) -> Work {
        let Some(foreground) = self.foreground.as_ref().filter(|f| !f.argv.is_empty()) else {
            return Work::Shell;
        };
        let argv = &foreground.argv;
        let program = basename(&argv[0]);
        let leader = foreground.pid.is_some() && foreground.pid == self.pid;
        if leader && SHELLS.contains(&program.trim_start_matches('-')) {
            return Work::Shell;
        }
        let agent = AgentKind::parse(program).or_else(|| {
            let app = self.status.as_ref()?.app.as_deref()?;
            AgentKind::parse(app).or_else(|| AgentKind::parse(app.strip_suffix("-code")?))
        });
        match agent {
            Some(agent) => Work::Agent {
                name: agent.name().to_owned(),
                argv: argv.clone(),
            },
            None => Work::Command(argv.clone()),
        }
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Each of `terminals` as its host's core reports it. One the core no
/// longer knows is left out, and its pane becomes an idle shell; any other
/// failure fails the review, so a running program is never taken for one.
pub(super) fn terminals(
    host: &Host,
    terminals: &[String],
    cancelled: &AtomicBool,
) -> Result<HashMap<String, Terminal>> {
    if terminals.is_empty() {
        return Ok(HashMap::new());
    }
    let body = terminals
        .iter()
        .map(|id| {
            format!(
                // herdr-core prints its errors as JSON on stdout too, so
                // only a record is passed on.
                "printf '\\036%s\\n' {id}\nif record=$(core_cli get --json {id}); then printf '%s\\n' \"$record\"; else status=$?; [ \"$status\" -eq {NOT_FOUND} ] || {{ printf '%s\\n' \"$record\" >&2; exit \"$status\"; }}; fi\n",
                id = shell_quote(id)
            )
        })
        .collect::<String>();
    let output = host.query(Step::Review, &body, &[], cancelled)?;
    parse_terminals(&String::from_utf8_lossy(&output))
}

/// One section per terminal: its ID, then its record, or nothing when the
/// core does not know it.
fn parse_terminals(output: &str) -> Result<HashMap<String, Terminal>> {
    let mut found = HashMap::new();
    for section in output.split('\u{1e}').filter(|section| !section.is_empty()) {
        let (id, json) = section.split_once('\n').unwrap_or((section, ""));
        let json = json.trim();
        if json.is_empty() {
            continue;
        }
        let terminal = serde_json::from_str::<Terminal>(json).map_err(decode(Step::Review))?;
        found.insert(id.to_owned(), terminal);
    }
    Ok(found)
}

/// Ask the agent in `terminal` for a handoff note and wait at most
/// `timeout` for its turn to end, as `herdr agent prompt --wait` does for a
/// Herdr 1 pane. The wait prints nothing until it ends, so the script may
/// stay quiet that long, and a little more for the core to answer.
pub(super) fn prompt(
    host: &Host,
    terminal: &str,
    prompt: &str,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<()> {
    let line = format!(
        "core_cli send --json --wait --timeout {} {} {} >/dev/null\n",
        timeout.as_secs(),
        shell_quote(terminal),
        shell_quote(prompt),
    );
    let limits = ScriptLimits {
        output: Host::QUERY_OUTPUT,
        idle: timeout + PROMPT_GRACE,
    };
    host.run(
        Step::Handoff,
        &line,
        &[][..],
        std::io::sink(),
        limits,
        cancelled,
    )
    .map(drop)
}

/// End the programs of `terminals`. Their tabs belong to the app, which
/// closes them; a terminal already gone counts as ended.
pub(super) fn stop(host: &Host, terminals: &[String], cancelled: &AtomicBool) -> Result<()> {
    if terminals.is_empty() {
        return Ok(());
    }
    let body = terminals
        .iter()
        .map(|id| {
            format!(
                "core_cli kill --json {id} >/dev/null || {{ status=$?; [ \"$status\" -eq {NOT_FOUND} ] || exit \"$status\"; }}\n",
                id = shell_quote(id)
            )
        })
        .collect::<String>();
    host.query(Step::Retire, &body, &[], cancelled).map(drop)
}

/// Whether `host` has a herdr-core to run tabs in.
pub(super) fn available(host: &Host, cancelled: &AtomicBool) -> Result<bool> {
    let output = host.query(
        Step::Review,
        "if core_cli --version >/dev/null 2>&1; then echo yes; fi\n",
        &[],
        cancelled,
    )?;
    Ok(output.trim_ascii() == b"yes")
}

/// What `herdr-core run --json` answers.
#[derive(Deserialize)]
struct Started {
    address: String,
}

/// Starts an idle shell in `host`'s core, in `cwd`, as the app's own new
/// tab does, and returns its terminal ID. A program is typed into it later
/// with [`type_line`], so its shell stays when it ends.
pub(super) fn start_shell(host: &Host, cwd: &str, cancelled: &AtomicBool) -> Result<String> {
    let line = format!(
        "core_cli run --bg --json --no-parent --cwd {}\n",
        shell_quote(cwd)
    );
    let output = host.query(Step::Tabs, &line, &[], cancelled)?;
    let started: Started =
        serde_json::from_slice(output.trim_ascii()).map_err(decode(Step::Tabs))?;
    Ok(started.address)
}

/// Types `line` into `terminal` and presses Enter, as `herdr pane run` does
/// for a Herdr 1 pane.
pub(super) fn type_line(
    host: &Host,
    terminal: &str,
    line: &str,
    cancelled: &AtomicBool,
) -> Result<()> {
    let body = format!(
        "core_cli send --json {} {} >/dev/null\n",
        shell_quote(terminal),
        shell_quote(line)
    );
    host.query(Step::Launch, &body, &[], cancelled).map(drop)
}

/// Terminals started on a destination for a move that has not finished:
/// dropped before [`Spawned::keep`], it ends them, since no tab would ever
/// show them.
pub(super) struct Spawned<'a> {
    host: &'a Host,
    ids: Vec<String>,
}

impl<'a> Spawned<'a> {
    pub(super) fn new(host: &'a Host) -> Self {
        Self {
            host,
            ids: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, id: String) {
        self.ids.push(id);
    }

    /// The move finished: the app opens their tabs.
    pub(super) fn keep(mut self) {
        self.ids.clear();
    }
}

impl Drop for Spawned<'_> {
    fn drop(&mut self) {
        if self.ids.is_empty() {
            return;
        }
        // Even a cancelled move ends what it started.
        if let Err(error) = stop(self.host, &self.ids, &AtomicBool::new(false)) {
            tracing::warn!(%error, "Could not end the Herdr 2 terminals of a failed teleport");
        }
    }
}

/// `node`, a source tab's split tree, as the app's panes of the terminals
/// that stand in for its panes on the destination.
pub(super) fn panes(node: &Node, terminals: &HashMap<String, String>) -> Option<Panes> {
    Panes::try_from(pane_node(node, terminals)?).ok()
}

fn pane_node(node: &Node, terminals: &HashMap<String, String>) -> Option<PaneNode> {
    Some(match node {
        Node::Pane(id) => PaneNode::Leaf(TerminalId::new(terminals.get(id)?.as_str()).ok()?),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => PaneNode::Split {
            axis: match direction {
                SplitDirection::Right => Axis::Row,
                SplitDirection::Down => Axis::Column,
            },
            share: Share::at(*ratio),
            first: Box::new(pane_node(first, terminals)?),
            second: Box::new(pane_node(second, terminals)?),
        },
    })
}

/// A tab's panes as the split tree Teleport rebuilds, keyed by terminal ID.
pub(super) fn tree(panes: &Panes) -> Node {
    node(panes.root())
}

fn node(pane: &PaneNode) -> Node {
    match pane {
        PaneNode::Leaf(id) => Node::Pane(id.as_str().to_owned()),
        PaneNode::Split {
            axis,
            share,
            first,
            second,
        } => Node::Split {
            direction: match axis {
                Axis::Row => SplitDirection::Right,
                Axis::Column => SplitDirection::Down,
            },
            ratio: share.fraction(),
            first: Box::new(node(first)),
            second: Box::new(node(second)),
        },
    }
}

/// Every terminal in `tabs`, in tab order.
pub(super) fn ids(tabs: &[Panes]) -> Vec<String> {
    tabs.iter()
        .flat_map(Panes::terminals)
        .map(|id| id.as_str().to_owned())
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
