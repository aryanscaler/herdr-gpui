//! The records herdr-core sends. Its API only grows, so unknown fields are
//! ignored and new words land in an `Other` variant rather than failing.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Terminal {
    /// `t7` here, `box/t7` on a machine this core added.
    pub address: String,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cols: u16,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub foreground: Option<Foreground>,
    #[serde(default)]
    pub keep: bool,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub pid: Option<i32>,
    /// Why it ended: `exited`, `killed`, `server_stop`, ...
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub rows: u16,
    pub state: TerminalState,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalState {
    Finished,
    Live,
    #[serde(other)]
    Other,
}

/// What runs in a terminal now: its foreground process group's leader.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Foreground {
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub name: String,
    pub pid: i32,
}

/// One OSC 7501 report, in the program's own words.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Status {
    #[serde(default)]
    pub app: String,
    /// `permission`, `question`, or `auth`, when blocked.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub msg: String,
    #[serde(default)]
    pub progress: Option<u8>,
    pub state: StatusState,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    Blocked,
    Done,
    Error,
    Idle,
    Working,
    #[serde(other)]
    Other,
}

/// The refusal kind of a send to a program that waits on the user.
pub const BLOCKED: &str = "blocked";

/// The refusal kind of a send whose `if` status no longer holds.
pub const STATUS_MISMATCH: &str = "status_mismatch";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct List {
    #[serde(default)]
    pub hidden_finished: u64,
    pub terminals: Vec<Terminal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Subscribed {
    #[serde(default)]
    pub channel: Option<u32>,
    pub seq: u64,
    pub terminals: Vec<Terminal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Attached {
    pub channel: u32,
    pub terminal: Terminal,
}

/// A stored document and the version to write over it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Doc {
    #[serde(default)]
    pub value: Value,
    /// Zero for a name never written.
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct DocSet {
    pub version: u64,
}

/// What the viewer looks like, so the core answers display queries for it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Display {
    /// `#rrggbb`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// Cell size in pixels: `[width, height]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Attach<'a> {
    pub cols: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<&'a Display>,
    /// Scrollback lines to paint first (main screen only).
    #[serde(skip_serializing_if = "is_zero")]
    pub history: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub observe: bool,
    pub rows: u16,
    pub terminal: &'a str,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Run<'a> {
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    pub argv: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<&'a str, &'a str>,
    /// Replace a lost program with a shell in place, never rerun it.
    pub keep: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<&'a str, &'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'a str>,
    /// Another machine this core added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<&'a str>,
}

/// Text for `terminal.send`: pasted once nobody has typed for a moment, then
/// submitted with Enter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SendText<'a> {
    /// Type even while the program is blocked on a dialog.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub force: bool,
    /// Type only while the program reports exactly this status (`idle` or
    /// `working`), checked by the core as it types; refused with
    /// [`STATUS_MISMATCH`] otherwise.
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub only_if: Option<&'a str>,
    pub terminal: &'a str,
    pub text: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Sent {
    /// Whether it went working within a few seconds; `None` when it reports
    /// no status, so nobody can tell.
    #[serde(default)]
    pub started: Option<bool>,
    /// The program's status word when the text went in.
    #[serde(default)]
    pub status: Option<String>,
    /// The turn when the text went in.
    pub turn: u64,
}

/// What `terminal.wait` saw: the program stopped working or ended, or the
/// wait checked in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Waited {
    /// False when the wait checked in: the program is still working.
    pub ended: bool,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub reason: WaitReason,
    /// The program's own status word, when it reports one.
    #[serde(default)]
    pub status: Option<String>,
    pub turn: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitReason {
    /// The program ended, or was lost in a kept terminal.
    Exit,
    /// A message reached the waiter.
    Message,
    /// After a send, the program did not start working.
    NotStarted,
    /// Output it waited for appeared.
    Output,
    /// The program reported a status other than working.
    Status,
    Timeout,
    #[serde(other)]
    Other,
}
