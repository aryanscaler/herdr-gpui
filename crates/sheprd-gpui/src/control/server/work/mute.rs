//! `worktree`: the mute of the caller's workspace. Mutes stay in this app;
//! nothing reaches the daemon.
use super::find;
use crate::{
    SheprdWindow,
    control::protocol::{
        Caller, ErrorCode, Response,
        work::{MuteInfo, Reply},
    },
    notification_mutes::{Daemon, Mutes, Scope},
};
use gpui::{App, Context};
use std::time::SystemTime;

pub(super) fn status(view: &SheprdWindow, caller: &Caller, cx: &App) -> Response {
    let found = match find(view, caller) {
        Ok(found) => found,
        Err(response) => return response,
    };
    let endpoint = &view.endpoints[found.index];
    let (muted, host_muted) = Mutes::of(cx).map_or((false, false), |mutes| {
        if let Some(herdr) = &found.herdr {
            let silenced = mutes.silenced(&Daemon::of(endpoint), herdr.snapshot);
            (silenced.workspace(&found.workspace.id), silenced.host())
        } else {
            let scope = crate::browser::scope(endpoint);
            (
                mutes.kept(scope.as_str(), Scope::Workspace, &found.workspace.id),
                mutes.host(crate::core_terminals::Host::of(&scope).endpoint()),
            )
        }
    });
    Response::Work {
        reply: Reply::Mute {
            mute: MuteInfo {
                host: endpoint.label.clone(),
                workspace: found.workspace.label.clone(),
                muted,
                host_muted,
            },
        },
    }
}

pub(super) fn set(
    view: &SheprdWindow,
    caller: &Caller,
    muted: bool,
    cx: &mut Context<SheprdWindow>,
) -> Response {
    let found = match find(view, caller) {
        Ok(found) => found,
        Err(response) => return response,
    };
    let daemon = Daemon::of(&view.endpoints[found.index]);
    let id = &found.workspace.id;
    let changed = if let Some(herdr) = &found.herdr {
        Mutes::update(cx, |mutes| {
            mutes.set(
                &daemon,
                herdr.snapshot,
                Scope::Workspace,
                id,
                muted,
                SystemTime::now(),
            )
        })
    } else {
        let scope = crate::browser::scope(&view.endpoints[found.index]);
        crate::core_terminals::mutes::set_kept(cx, scope.as_str(), Scope::Workspace, id, muted)
    };
    if changed {
        cx.notify();
    }
    let response = status(view, caller, cx);
    match &response {
        Response::Work {
            reply: Reply::Mute { mute },
        } if mute.muted != muted => Response::error(
            ErrorCode::InvalidRequest,
            "This workspace's ID is too long to keep a mute on",
        ),
        _ => response,
    }
}
