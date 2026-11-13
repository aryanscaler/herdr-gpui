//! The remote-label boundary never impersonates a local socket caller.
use super::{Source, find, flows, mute, notes, worktrees};
use crate::{
    SheprdWindow,
    browser::{Location, Store},
    control::protocol::{
        ErrorCode, Response,
        work::{WorkOp, remote::Request},
    },
    core_terminals::Host,
    remote_work::Target,
};
use gpui::{Context, Window};

impl SheprdWindow {
    pub(crate) fn serve_remote_work(
        &mut self,
        target: Target<'_>,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Response {
        let Target {
            scope,
            tab,
            terminal,
            workspace,
        } = target;
        if request
            .window
            .is_some_and(|id| id != window.window_handle().window_id().as_u64())
        {
            return Response::error(
                ErrorCode::WorkspaceNotFound,
                "The requested window does not hold this caller",
            );
        }
        let Some(stored) = cx.try_global::<Store>().and_then(|store| store.get(tab)) else {
            return Response::error(ErrorCode::WorkspaceNotFound, "The caller's tab is gone");
        };
        let driven = matches!(&stored.location, Some(Location::CoreTerminal { panes, observe: false, .. }) if panes.contains(terminal));
        if scope.is_local()
            || stored.scope != *scope
            || stored.workspace_id != workspace
            || !driven
            || self.core_terminals.host_of(tab) != Host::Remote(scope.clone())
            || !self.core_terminals.tab_terminals(tab).contains(&terminal)
        {
            return Response::error(
                ErrorCode::WorkspaceNotFound,
                "This window does not drive the caller's terminal on its host",
            );
        }
        let workspace = stored.workspace_id.clone();
        let Some(index) = self
            .endpoints
            .iter()
            .position(|endpoint| crate::browser::scope(endpoint) == *scope)
        else {
            return Response::error(ErrorCode::WorkspaceNotFound, "The caller's host is gone");
        };
        let source = Source::Remote {
            index,
            scope,
            workspace: &workspace,
        };
        // Resolve every operation, including reads, before accessing app data.
        let found = match find(self, source) {
            Ok(found) => found,
            Err(response) => return response,
        };
        let checkout = notes::checkout_of(&self.endpoints[index].id, &found.workspace);
        let dialog = matches!(
            &request.operation,
            WorkOp::Restore { .. }
                | WorkOp::Teleport { .. }
                | WorkOp::FanOut(_)
                | WorkOp::KeptCreate { .. }
                | WorkOp::KeptOpen {}
                | WorkOp::KeptDelete {}
        );
        if dialog && self.menu.page.is_some() {
            return Response::error(
                ErrorCode::WindowBusy,
                "Close the window's existing dialog first",
            );
        }
        match request.operation {
            WorkOp::Notes => notes::list_on(&self.endpoints[index].id, cx),
            WorkOp::Note | WorkOp::SetNote { .. } if checkout.is_none() => Response::error(
                ErrorCode::InvalidRequest,
                "Notes need a Git checkout on a branch",
            ),
            WorkOp::Note => checkout.map_or_else(
                || Response::error(ErrorCode::InvalidRequest, "Missing checkout"),
                |checkout| notes::get_at(&checkout, cx),
            ),
            WorkOp::SetNote { text } => checkout.map_or_else(
                || Response::error(ErrorCode::InvalidRequest, "Missing checkout"),
                |checkout| notes::set_at(checkout, &text, cx),
            ),
            WorkOp::MuteStatus => mute::status(self, source, cx),
            WorkOp::Mute { muted } => mute::set(self, source, muted, cx),
            WorkOp::Restore { id } => flows::restore(self, source, id, window, cx),
            WorkOp::Script { kind } => flows::script(self, source, kind, cx),
            WorkOp::Teleport { host } => flows::teleport(self, source, &host, window, cx),
            WorkOp::FanOut(request) => flows::fan_out(self, source, request, window, cx),
            WorkOp::KeptList {} => worktrees::list(self, source),
            WorkOp::KeptCreate { branch } => {
                worktrees::dialog(self, source, worktrees::Dialog::Create(branch), window, cx)
            }
            WorkOp::KeptOpen {} => {
                worktrees::dialog(self, source, worktrees::Dialog::Open, window, cx)
            }
            WorkOp::KeptDelete {} => {
                worktrees::dialog(self, source, worktrees::Dialog::Delete, window, cx)
            }
        }
    }
}
