//! App-kept worktree commands use the same host-scoped dialogs as the menu.
use super::{Source, find, flows::bring_forward, selected_workspace};
use crate::{
    SheprdWindow,
    control::protocol::{
        ErrorCode, Response,
        work::{Reply, WorktreeInfo},
    },
    menu::{Page, WorkspaceAction},
};
use gpui::{Context, Point, Window};

pub(super) enum Dialog {
    Create(Option<String>),
    Open,
    Delete,
}

pub(super) fn list<'a>(view: &SheprdWindow, caller: impl Into<Source<'a>>) -> Response {
    let found = match find(view, caller) {
        Ok(found) => found,
        Err(error) => return error,
    };
    let endpoint = &view.endpoints[found.index];
    let worktrees = view
        .sessions(found.index)
        .core
        .as_ref()
        .map(|core| {
            core.workspaces
                .iter()
                .filter_map(|workspace| {
                    Some(WorktreeInfo {
                        host: endpoint.id.clone(),
                        workspace: workspace.id.clone(),
                        path: workspace.cwd.clone(),
                        branch: workspace.branch.clone(),
                        linked: workspace.worktree.as_ref()?.linked,
                    })
                })
                .take(512)
                .collect()
        })
        .unwrap_or_default();
    Response::Work {
        reply: Reply::Worktrees { worktrees },
    }
}

pub(super) fn dialog<'a>(
    view: &mut SheprdWindow,
    caller: impl Into<Source<'a>>,
    dialog: Dialog,
    window: &mut Window,
    cx: &mut Context<SheprdWindow>,
) -> Response {
    let id = match selected_workspace(view, caller) {
        Ok(id) => id,
        Err(error) => return error,
    };
    if !view.is_kept_workspace(&id) {
        return Response::error(
            ErrorCode::Unsupported,
            "Use Herdr's worktree command for a Herdr 1 workspace",
        );
    }
    if let Dialog::Create(Some(branch)) = &dialog
        && let Err(error) = crate::worktree::validate_branch(branch)
    {
        return Response::error(ErrorCode::InvalidRequest, error.to_string());
    }
    let sessions = view.sessions(view.selected_endpoint);
    let tree = sessions
        .core
        .as_ref()
        .and_then(|core| core.workspace(&id))
        .and_then(|workspace| workspace.worktree.as_ref());
    let delete = matches!(dialog, Dialog::Delete);
    if tree.is_none() || (delete && tree.is_none_or(|tree| !tree.linked)) {
        return Response::error(
            ErrorCode::InvalidRequest,
            "This operation needs a Git checkout; deletion requires a linked worktree",
        );
    }
    let action = if delete {
        WorkspaceAction::DeleteWorktree
    } else {
        WorkspaceAction::NewWorktree
    };
    view.open_workspace_dialog_for(&id, action, Point::default(), window, cx);
    if view.menu.page != Some(Page::Dialog(action)) {
        view.dismiss_menu(window, cx);
        return Response::error(ErrorCode::Failed, "The worktree dialog did not open");
    }
    match dialog {
        Dialog::Create(Some(branch)) => {
            if let Some(input) = &mut view.menu.input {
                *input = crate::dialog_input::DialogInput::branch(branch);
            }
        }
        Dialog::Open => view.show_existing_worktrees(window, cx),
        _ => {}
    }
    bring_forward(window, cx);
    Response::done("Opened the worktree dialog; review and confirm in Sheprd")
}
