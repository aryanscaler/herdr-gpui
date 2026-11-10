//! Opening a fan-out from a workspace row: the repository's main checkout
//! creates every lane's worktree, branched from the row's own branch when the
//! row is a linked checkout. A workspace the app keeps for Herdr 2 makes its
//! lanes with Git through its own folder, on this computer.

// i18n: migrated
use super::{
    Page, WorkspaceTarget,
    workspace::{Source as Addressed, popover::Label},
};
use crate::{
    SheprdWindow,
    dialog_input::DialogInput,
    fan_out::{FanOut, Journal, Origin},
    teleport::{Host, host_for},
};
use gpui::{Context, Window};

/// What a fan-out branches from.
struct Source {
    workspace_id: String,
    repo_key: String,
    repo_label: String,
    base: String,
    /// A kept workspace's folder; see [`Origin::checkout`].
    checkout: Option<String>,
}

impl SheprdWindow {
    /// The repository checkout new lanes are created through, when the menu's
    /// workspace has one: its workspace, repository key and label, and base
    /// ref.
    fn fan_out_source(&self) -> Option<Source> {
        let target = self.menu.target.as_ref()?;
        let describe = |source: &WorkspaceTarget| {
            let tree = source.worktree.as_ref();
            Source {
                workspace_id: source.id.clone(),
                repo_key: tree.map(|tree| tree.key.clone()).unwrap_or_default(),
                repo_label: tree.map_or_else(|| source.label.clone(), |tree| tree.label.clone()),
                base: source.base_ref().unwrap_or_else(|_| "HEAD".to_owned()),
                checkout: None,
            }
        };
        // A kept workspace whose folder Git names branches through that
        // folder, whichever checkout of the repository it is.
        if target.keeper == sheprd_session::Keeper::App {
            target.worktree.as_ref()?;
            let checkout = self.kept_checkout()?;
            // A linked checkout's lanes start from its branch, which the
            // base commit is read through on the repository.
            let base = match target.branch.as_deref() {
                Some(branch) if crate::worktree::validate_branch(branch).is_ok() => {
                    format!("refs/heads/{branch}")
                }
                _ => "HEAD".to_owned(),
            };
            return Some(Source {
                base,
                checkout: Some(checkout),
                ..describe(target)
            });
        }
        // Lanes are created through the workspace, so one whose shell left
        // the repository Herdr keeps for it would branch that repository.
        if target.can_create() {
            return (target.source == Addressed::Workspace).then(|| describe(target));
        }
        self.linked_new_worktree_target()
            .filter(|target| target.source == Addressed::Workspace)
            .as_ref()
            .map(describe)
    }

    /// Whether the workspace a fan-out creates its lanes through is still in
    /// the repository the fan-out was opened for. Its shell may have moved
    /// since, while Herdr keeps the old repository for it.
    pub(crate) fn fan_out_origin_current(&self) -> bool {
        let Some(origin) = self.fan_out.as_ref().map(FanOut::origin) else {
            return false;
        };
        // The app keeps the workspace; Git reads its folder at launch.
        if origin.kept() {
            return self.is_kept_workspace(&origin.workspace_id);
        }
        let Some(index) = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.id == origin.endpoint_id)
        else {
            return false;
        };
        self.endpoint_snapshot(index)
            .and_then(|snapshot| {
                snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == origin.workspace_id)
            })
            .is_some_and(
                |workspace| match super::workspace::known_worktree(workspace) {
                    Some(tree) => tree.key == origin.repo_key,
                    // A branch-only workspace has no key to compare.
                    None => workspace.worktree.is_none() && origin.repo_key.is_empty(),
                },
            )
    }

    /// The workspace menu's fan-out row: the comparison of a launched fan-out
    /// while there is one, otherwise a new fan-out from a Git workspace on a
    /// host that can be scripted.
    pub(crate) fn fan_out_item(&self) -> Option<Label> {
        if self
            .fan_out
            .as_ref()
            .is_some_and(|fan_out| !fan_out.composing())
        {
            return Some(Label::FAN_OUT_COMPARISON);
        }
        // Host scripts need a POSIX client; see `sheprd_client::run_script`.
        let scriptable = cfg!(any(target_os = "linux", target_os = "macos"));
        let source = self.fan_out_source()?;
        (scriptable && self.fan_out_host(&source).is_some()).then_some(Label::FAN_OUT)
    }

    /// The host `source`'s lanes are made on: the shown one, or this
    /// computer for a workspace the app keeps, which is always here.
    fn fan_out_host(&self, source: &Source) -> Option<Host> {
        if source.checkout.is_some() {
            return Host::new(&sheprd_client::ConnectTarget::Local).ok();
        }
        host_for(&self.endpoints[self.selected_endpoint].connection.target).ok()
    }

    pub(crate) fn open_fan_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .fan_out
            .as_ref()
            .is_some_and(|fan_out| !fan_out.composing())
        {
            self.menu.page = Some(Page::FanOut);
            self.menu.error = None;
            self.menu.input = None;
            window.focus(&self.menu.focus, cx);
            cx.notify();
            return;
        }
        let Some(source) = self.fan_out_source() else {
            return;
        };
        let Some(host) = self.fan_out_host(&source) else {
            return;
        };
        let Source {
            workspace_id,
            repo_key,
            repo_label,
            base,
            checkout,
        } = source;
        let selected = &self.endpoints[self.selected_endpoint];
        let origin = Origin {
            endpoint_id: selected.id.clone(),
            endpoint_label: selected.label.clone(),
            host,
            workspace_id,
            repo_key,
            repo_label,
            base,
            checkout,
        };
        self.menu.page = Some(Page::FanOut);
        self.menu.error = None;
        self.menu.input = Some(DialogInput::default());
        self.fan_out = Some(FanOut::start(origin, Journal::get(cx)));
        window.focus(&self.menu.focus, cx);
        cx.notify();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
