//! Sub-agents of agents that speak ACP directly, which ACP has no word for.
//! Copilot's and OpenCode's show as a tool call whose input names the agent
//! and its prompt. OpenCode also sends the child session's own updates on the
//! parent session, each marked in `_meta`. This goes by input neither agent
//! promises to keep, so a bridge's tag always wins over it.
//!
//! This is the fallback when no child lifecycle extension was negotiated.
//! OpenCode peers advertising `opencode/child-session-updates` instead use
//! `super::opencode`, including background work and concurrent child steps.
//! On this fallback only a sub-agent the call waits for is followed: one sent to the
//! background returns at once, and its end cannot be known on this path, so
//! it stays an ordinary card rather than an activity that never ends. Nothing
//! here can stop one, as ACP has no request for it.

use acp::v1 as wire;
use serde_json::Value;

/// The `_meta` key OpenCode marks a child session's updates with.
const CHILD: &str = "opencode/child-session";
/// The input keys that name the agent a call starts: Copilot's, OpenCode's
/// `task`, and its newer `subagent`.
const AGENT: [&str; 3] = ["agent_type", "subagent_type", "agent"];

/// Whether a call with `input` runs a sub-agent and waits for it.
pub(super) fn waits_for_one(input: &Value) -> bool {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let background = input.get("background").and_then(Value::as_bool) == Some(true)
        || input.get("run_in_background").and_then(Value::as_bool) == Some(true)
        || text("mode") == Some("background");
    !background && text("prompt").is_some() && AGENT.iter().any(|key| text(key).is_some())
}

/// Whether an update comes from a child session rather than the conversation.
pub(super) fn from_child(meta: Option<&wire::Meta>) -> bool {
    meta.is_some_and(|meta| meta.contains_key(CHILD))
}

#[cfg(test)]
mod tests;
