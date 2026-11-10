//! `notes`, `worktree`, `checkpoint restore`, `scripts`, `teleport`, and
//! `fan-out`.
//!
//! Every command acts on the caller's checkout: the workspace of the caller's
//! pane on the caller's daemon, else the workspace the window focuses. A
//! checkout is named by endpoint, repository (Git common directory), and
//! branch, never by workspace ID, which does not survive a daemon restart.
//! Flows that change things for real only open the app's own dialog, already
//! pointed at that checkout; the user confirms them there.
mod flows;
mod mute;
mod notes;
#[cfg(test)]
mod tests;

pub(super) use flows::bring_forward;

use super::{Watches, in_free_window, in_window, not_found, socket, target};
use crate::{
    SheprdWindow,
    control::protocol::{Call, Caller, ErrorCode, Response, work::WorkOp},
};
use gpui::App;
use sheprd_client::protocol::{ClientShellSnapshot, ClientShellWorkspace};

pub(super) fn serve(call: Call<WorkOp>, incoming: socket::Incoming, _: &mut Watches, cx: &mut App) {
    let Call { caller, op } = call;
    let response = match op {
        WorkOp::Notes => notes::list(cx),
        WorkOp::Note => notes::get(&caller, cx),
        WorkOp::SetNote { text } => notes::set(&caller, &text, cx),
        WorkOp::MuteStatus => in_window(&caller, cx, |view, _, cx| mute::status(view, &caller, cx)),
        WorkOp::Mute { muted } => in_window(&caller, cx, |view, _, cx| {
            mute::set(view, &caller, muted, cx)
        }),
        WorkOp::Restore { id } => in_free_window(&caller, cx, |view, window, cx| {
            flows::restore(view, &caller, id, window, cx)
        }),
        WorkOp::Script { kind } => in_window(&caller, cx, |view, _, cx| {
            flows::script(view, &caller, kind, cx)
        }),
        WorkOp::Teleport { host } => in_free_window(&caller, cx, |view, window, cx| {
            flows::teleport(view, &caller, &host, window, cx)
        }),
        WorkOp::FanOut(request) => in_free_window(&caller, cx, |view, window, cx| {
            flows::fan_out(view, &caller, request, window, cx)
        }),
    };
    incoming.respond(response);
}

/// The caller's workspace as one window shows it.
pub(super) struct Found<'a> {
    /// The endpoint showing it, by index into the window's endpoints.
    pub index: usize,
    /// The workspace as the session model has it, whichever keeps it.
    pub workspace: sheprd_session::Workspace,
    /// Herdr 1's state of it; `None` for a workspace the app keeps for
    /// Herdr 2, which has none of what mutes, scripts, and fan-out keep.
    pub herdr: Option<Herdr<'a>>,
}

/// A workspace as Herdr 1's snapshot holds it.
pub(super) struct Herdr<'a> {
    pub snapshot: &'a ClientShellSnapshot,
    pub workspace: &'a ClientShellWorkspace,
}

impl<'a> Found<'a> {
    /// Herdr 1's state, or why a workspace the app keeps cannot do `what`.
    pub(super) fn herdr(&self, what: &str) -> Result<&Herdr<'a>, Response> {
        self.herdr.as_ref().ok_or_else(|| {
            Response::error(
                ErrorCode::Unsupported,
                format!(
                    "Workspace {} is a Herdr 2 workspace, where {what} is not available yet",
                    self.workspace.label
                ),
            )
        })
    }
}

/// Finds the caller's workspace in `view`: on the caller's own daemon only
/// when it names one, else on any endpoint showing a workspace of that ID. A
/// caller in a herdr-core terminal is in the workspace of the tab holding
/// it: Herdr 1's, or one the app keeps for Herdr 2.
pub(super) fn find<'a>(view: &'a SheprdWindow, caller: &Caller) -> Result<Found<'a>, Response> {
    let target = target(caller);
    let (index, id) = target
        .strictness()
        .iter()
        .find_map(|strict| view.control_target(&target, *strict))
        .ok_or_else(|| match &caller.workspace_id {
            Some(id) => Response::error(
                ErrorCode::WorkspaceNotFound,
                format!("No window shows workspace {id}"),
            ),
            None => not_found("The window shows no workspace"),
        })?;
    let sessions = view.sessions(index);
    if let Some(kept) = sessions.core.as_ref().and_then(|core| core.workspace(&id)) {
        return Ok(Found {
            index,
            workspace: kept.clone(),
            herdr: None,
        });
    }
    let live = if index == view.selected_endpoint {
        &view.live
    } else {
        &view.endpoints[index].live
    };
    // A herdr-core terminal outside every workspace, as one in a tab
    // another window placed, has none of Herdr's state.
    let unsupported = || {
        let terminal = caller
            .terminal_id
            .as_deref()
            .filter(|_| caller.workspace_id.is_none())?;
        Some(Response::error(
            ErrorCode::Unsupported,
            format!("herdr-core terminal {terminal} is in no workspace this window keeps"),
        ))
    };
    let snapshot = live.snapshot.as_deref().ok_or_else(|| {
        unsupported().unwrap_or_else(|| not_found("The window has no state from that host yet"))
    })?;
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == id)
        .ok_or_else(|| {
            unsupported().unwrap_or_else(|| not_found(format!("No window shows workspace {id}")))
        })?;
    let session = sessions
        .herdr_1
        .as_ref()
        .and_then(|session| session.workspace(&id))
        .cloned()
        .ok_or_else(|| not_found(format!("No window shows workspace {id}")))?;
    Ok(Found {
        index,
        workspace: session,
        herdr: Some(Herdr {
            snapshot,
            workspace,
        }),
    })
}

/// The caller's workspace ID, when it is on the window's selected host:
/// dialogs act through that host alone, and a Herdr 1 workspace only while
/// its daemon is connected.
pub(super) fn selected_workspace(view: &SheprdWindow, caller: &Caller) -> Result<String, Response> {
    let found = find(view, caller)?;
    if found.index != view.selected_endpoint {
        return Err(Response::error(
            ErrorCode::InvalidRequest,
            format!(
                "Workspace {} is on host {}, which the window is not showing; select it first",
                found.workspace.id, view.endpoints[found.index].label
            ),
        ));
    }
    if found.herdr.is_some() && !view.live.status.is_connected() {
        return Err(Response::error(
            ErrorCode::Busy,
            "Sheprd is not connected to that host right now",
        ));
    }
    Ok(found.workspace.id)
}

/// The repository a workspace's checkout belongs to, for messages.
pub(super) fn repository(workspace: &sheprd_session::Workspace) -> &str {
    workspace
        .worktree
        .as_ref()
        .map_or(workspace.label.as_str(), |tree| tree.label.as_str())
}
