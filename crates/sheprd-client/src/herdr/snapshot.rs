//! Typed views of `herdr` CLI output. The GUI connection's snapshot carries no
//! split tree, launch argv, or agent session, so Teleport reads the socket API
//! snapshot through the CLI on each host. Only the fields used are decoded;
//! unknown fields are ignored so newer daemons stay readable.

use serde::Deserialize;

/// `{"id": .., "result": T}`, as every `herdr` API subcommand prints it.
#[derive(Debug, Deserialize)]
pub struct Envelope<T> {
    pub result: T,
}

#[derive(Debug, Deserialize)]
pub struct SnapshotResult {
    pub snapshot: HostSnapshot,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HostSnapshot {
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
    #[serde(default)]
    pub layouts: Vec<Layout>,
}

impl HostSnapshot {
    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.workspace_id == id)
    }

    /// The workspace's tabs in their sidebar order.
    pub fn tabs_of(&self, workspace_id: &str) -> Vec<&Tab> {
        let mut tabs: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == workspace_id)
            .collect();
        tabs.sort_by_key(|tab| tab.number);
        tabs
    }

    pub fn pane(&self, id: &str) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.pane_id == id)
    }

    pub fn layout(&self, tab_id: &str) -> Option<&Layout> {
        self.layouts.iter().find(|layout| layout.tab_id == tab_id)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    pub worktree: Option<Worktree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Worktree {
    pub repo_key: String,
    pub repo_name: String,
    pub repo_root: String,
    pub checkout_path: String,
    pub is_linked_worktree: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub number: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pane {
    pub pane_id: String,
    pub tab_id: String,
    pub cwd: Option<String>,
    pub foreground_cwd: Option<String>,
    pub agent: Option<String>,
    pub agent_session: Option<AgentSession>,
    /// What the daemon reports the pane's agent doing.
    #[serde(default, rename = "agent_status", deserialize_with = "pane_status")]
    pub status: PaneStatus,
}

impl Pane {
    /// The daemon reports the pane's agent as working.
    pub fn working(&self) -> bool {
        self.status == PaneStatus::Working
    }
}

/// What a pane's agent is doing, as far as a caller here needs to tell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PaneStatus {
    Done,
    Idle,
    Working,
    /// Waiting for an answer, unknown to the daemon, or a status this client
    /// does not know, so a new one cannot break parsing.
    #[default]
    Other,
}

fn pane_status<'de, D: serde::Deserializer<'de>>(d: D) -> Result<PaneStatus, D::Error> {
    Ok(match Option::<String>::deserialize(d)?.as_deref() {
        Some("done") => PaneStatus::Done,
        Some("idle") => PaneStatus::Idle,
        Some("working") => PaneStatus::Working,
        _ => PaneStatus::Other,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct AgentSession {
    pub agent: String,
    pub kind: SessionKind,
    pub source: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Id,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layout {
    pub tab_id: String,
    pub panes: Vec<LayoutPane>,
    pub splits: Vec<LayoutSplit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayoutPane {
    pub pane_id: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Right,
    Down,
}

impl SplitDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayoutSplit {
    /// `split_<n>_root` or `split_<n>_<path>`, where the path spells the
    /// branch from the root: `0` is the first child, `1` the second.
    pub id: String,
    pub direction: SplitDirection,
    pub ratio: f32,
    pub rect: Rect,
}

#[derive(Debug, Deserialize)]
pub struct ProcessInfoResult {
    pub process_info: ProcessInfo,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProcessInfo {
    pub foreground_process_group_id: Option<u32>,
    pub shell_pid: Option<u32>,
    #[serde(default)]
    pub foreground_processes: Vec<Process>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Process {
    pub pid: u32,
    #[serde(default)]
    pub argv: Vec<String>,
    pub cwd: Option<String>,
}

impl ProcessInfo {
    /// Every process in the foreground job; none while the shell itself is in
    /// the foreground (an idle prompt).
    pub fn foreground_pids(&self) -> Vec<u32> {
        if self.foreground_process_group_id.is_none()
            || self.foreground_process_group_id == self.shell_pid
        {
            return Vec::new();
        }
        self.foreground_processes.iter().map(|p| p.pid).collect()
    }

    /// The foreground job's leader, or `None` while the shell itself is in
    /// the foreground (an idle prompt).
    pub fn foreground_job(&self) -> Option<&Process> {
        let group = self.foreground_process_group_id?;
        if Some(group) == self.shell_pid {
            return None;
        }
        self.foreground_processes
            .iter()
            .find(|process| process.pid == group)
            .or_else(|| self.foreground_processes.iter().min_by_key(|p| p.pid))
            .filter(|process| !process.argv.is_empty())
    }
}

/// `worktree_created`: the new workspace and the pane its first tab holds.
#[derive(Debug, Deserialize)]
pub struct WorktreeCreated {
    pub workspace: CreatedWorkspace,
    pub tab: CreatedTab,
    pub root_pane: CreatedPane,
    pub worktree: CreatedWorktree,
}

#[derive(Debug, Deserialize)]
pub struct CreatedWorkspace {
    pub workspace_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatedTab {
    pub tab_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatedPane {
    pub pane_id: String,
    /// Stable identity for callers that must not retry against a replaced pane.
    pub terminal_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatedWorktree {
    pub path: String,
}

/// `workspace_created`.
#[derive(Debug, Deserialize)]
pub struct WorkspaceCreated {
    pub workspace: CreatedWorkspace,
}

/// `tab_list`.
#[derive(Debug, Deserialize)]
pub struct TabList {
    pub tabs: Vec<CreatedTab>,
}

/// `tab_created`.
#[derive(Debug, Deserialize)]
pub struct TabCreated {
    pub tab: CreatedTab,
    pub root_pane: CreatedPane,
}

/// `pane_info`, as `pane split` prints the new pane.
#[derive(Debug, Deserialize)]
pub struct PaneInfoResult {
    pub pane: CreatedPane,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
