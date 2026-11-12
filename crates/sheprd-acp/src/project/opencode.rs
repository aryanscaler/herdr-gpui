//! OpenCode's negotiated ACP child stream, independent of parent prompt turns.
//! Contract: packages/cli/src/acp/{capabilities,child,translate,turn}.ts in
//! anomalyco/opencode. Launch-tool completion is deliberately not a lifecycle.

use acp::v1 as wire;
use serde::Deserialize;
use serde_json::Value;
use sheprd_chat_protocol::{
    ActivityKind, ActivityState, Change, MAX_ACTIVITY_TEXT, ToolKind, ToolStatus, Transcript,
};

use super::{Projector, Report, Tag, activity, apply_call, apply_fields};

pub(crate) const CAPABILITY: &str = "opencode/child-session-updates";
pub(crate) const METHOD: &str = "opencode/session/child_update";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Child {
    root_session_id: String,
    child_session_id: String,
    parent_session_id: String,
    depth: u32,
    title: Option<String>,
    #[serde(flatten)]
    event: Event,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Status {
        status: Status,
        error: Option<Failure>,
    },
    Update {
        update: Box<wire::SessionUpdate>,
    },
}

#[derive(Deserialize)]
struct Failure {
    message: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Created,
    Running,
    Completed,
    Failed,
    Interrupted,
}

fn activity_id(session: &str) -> String {
    format!("opencode:{session}")
}

impl Projector {
    pub(crate) fn opencode_child(
        &mut self,
        transcript: &Transcript,
        root: &str,
        params: Value,
        now: u64,
    ) -> Vec<Change> {
        if !self.opencode_children {
            return Vec::new();
        }
        let Ok(child) = serde_json::from_value::<Child>(params) else {
            return Vec::new();
        };
        let id = activity_id(&child.child_session_id);
        if child.root_session_id != root
            || child.child_session_id.is_empty()
            || child.child_session_id == root
            || id.len() > MAX_ACTIVITY_TEXT
            || child.depth == 0
            || child.parent_session_id.is_empty()
            || child.parent_session_id == child.child_session_id
            || (child.depth == 1 && child.parent_session_id != root)
            || (child.depth > 1 && child.parent_session_id == root)
        {
            return Vec::new();
        }
        match child.event {
            Event::Status { status, error } => {
                // Continuing a child session is a new execution. Only an
                // explicit running event can reopen it, never a late creation
                // or a launch tool's result.
                let restarted = if matches!(status, Status::Running)
                    && let Some(known) = transcript.activity(&id).filter(|a| !a.is_running())
                {
                    let mut activity = known.clone();
                    activity.state = ActivityState::Running;
                    activity.started_ms = now;
                    activity.finished_ms = None;
                    activity.detail = None;
                    Some(Change::Activity { activity })
                } else {
                    None
                };
                let state = match status {
                    Status::Created | Status::Running => ActivityState::Running,
                    Status::Completed => ActivityState::Finished,
                    Status::Failed => ActivityState::Failed,
                    Status::Interrupted => ActivityState::Killed,
                };
                let mut report = Report::new(ActivityKind::Subagent, state);
                report.detail = error.map(|error| error.message);
                let restarting = restarted.is_some();
                let change = restarted.or_else(|| {
                    activity::record(
                        transcript,
                        &id,
                        child.title.as_deref().unwrap_or(&child.child_session_id),
                        report,
                        now,
                    )
                });
                let Some(change) = change else {
                    return Vec::new();
                };
                // The rail links to this session's card. Launch call IDs are
                // unrelated and may represent several executions of a child.
                let card = self.tool(transcript, &id, |tool| {
                    tool.kind = ToolKind::Subagent;
                    tool.title =
                        activity::clip(child.title.as_deref().unwrap_or(&child.child_session_id));
                    tool.status = match state {
                        ActivityState::Running => ToolStatus::InProgress,
                        ActivityState::Finished => ToolStatus::Completed,
                        _ => ToolStatus::Failed,
                    };
                    if tool.started_ms.is_none() || restarting {
                        tool.started_ms = Some(now);
                    }
                });
                vec![card, change]
            }
            Event::Update { update } => {
                // Only child tools belong in the activity's steps. Child text,
                // plans, usage, and settings must never replace the parent's.
                if transcript.activity(&id).is_none() {
                    return Vec::new();
                }
                let tag = Tag {
                    activity: None,
                    parent: Some(id),
                };
                let prefix = format!("{}:", child.child_session_id);
                match *update {
                    wire::SessionUpdate::ToolCall(call)
                        if call.tool_call_id.0.starts_with(&prefix) =>
                    {
                        let call_id = call.tool_call_id.0.to_string();
                        self.tagged(transcript, &call_id, &tag, now, |tool| {
                            apply_call(tool, call)
                        })
                    }
                    wire::SessionUpdate::ToolCallUpdate(update)
                        if update.tool_call_id.0.starts_with(&prefix) =>
                    {
                        let call_id = update.tool_call_id.0.to_string();
                        self.tagged(transcript, &call_id, &tag, now, |tool| {
                            apply_fields(tool, update.fields);
                        })
                    }
                    _ => Vec::new(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
