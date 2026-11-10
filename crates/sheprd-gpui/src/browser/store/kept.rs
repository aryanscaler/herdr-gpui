//! The workspaces the app keeps for Herdr 2 terminals. herdr-core has no
//! workspaces, so the app keeps each one's name, folder, and place in the
//! user's order beside its tabs, in the same file the remote control daemon
//! reads (`sheprd_session::SavedTabs`).

use super::{Scope, Store, Tab};
use crate::reorder::Beside;
use sheprd_session::{
    SavedWorkspace,
    saved::{MAX_CWD_BYTES, MAX_LABEL_CHARS},
};

/// The most workspaces the app keeps, across every host.
pub(crate) const MAX_WORKSPACES: usize = 64;
/// How the app's workspace IDs start, apart from Herdr 1's and the one a
/// window without Herdr 1 gathered its tabs in before the app kept any.
const PREFIX: &str = "space-";

impl Store {
    /// `scope`'s kept workspaces, in the user's order.
    pub(crate) fn kept_workspaces<'a>(
        &'a self,
        scope: &'a Scope,
    ) -> impl Iterator<Item = &'a SavedWorkspace> + 'a {
        self.workspaces
            .iter()
            .filter(move |workspace| workspace.scope == scope.as_str())
    }

    /// Every kept workspace, by its scope and ID.
    pub(crate) fn kept_ids(&self) -> impl Iterator<Item = (&str, &str)> {
        self.workspaces
            .iter()
            .map(|workspace| (workspace.scope.as_str(), workspace.id.as_str()))
    }

    pub(crate) fn kept_workspace(&self, scope: &Scope, id: &str) -> Option<&SavedWorkspace> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.scope == scope.as_str() && workspace.id == id)
    }

    /// How many more workspaces the app can keep, across every host.
    pub(crate) fn kept_room(&self) -> usize {
        MAX_WORKSPACES.saturating_sub(self.workspaces.len())
    }

    /// Keeps a new workspace at the end of `scope`'s, and answers its ID, or
    /// `None` when the app already keeps the most it does. `custom` when the
    /// user gave its name, rather than the app taking its folder's.
    pub(crate) fn keep_workspace(
        &mut self,
        scope: &Scope,
        label: &str,
        cwd: &str,
        custom: bool,
    ) -> Option<String> {
        // Past every ID kept or handed out before, so a closed workspace's
        // ID is never given to another while a late answer may still name it.
        let next = self
            .workspaces
            .iter()
            .filter_map(|workspace| workspace.id.strip_prefix(PREFIX)?.parse::<u64>().ok())
            .chain(std::iter::once(self.last_kept))
            .max()
            .unwrap_or(0)
            + 1;
        let id = format!("{PREFIX}{next}");
        let kept = self.keep_as(scope, &id, label, cwd, custom);
        if kept {
            self.last_kept = next;
        }
        kept.then_some(id)
    }

    /// Keeps `id`, unless it is kept already, as the workspace a window with
    /// no Herdr 1 installed gathers its tabs in.
    pub(crate) fn keep_default(&mut self, scope: &Scope, id: &str, label: &str) -> bool {
        self.kept_workspace(scope, id).is_none() && self.keep_as(scope, id, label, "", false)
    }

    fn keep_as(&mut self, scope: &Scope, id: &str, label: &str, cwd: &str, custom: bool) -> bool {
        let workspace = SavedWorkspace {
            cwd: cwd.to_owned(),
            custom,
            id: id.to_owned(),
            label: clean_label(label),
            scope: scope.as_str().to_owned(),
        };
        if self.workspaces.len() >= MAX_WORKSPACES
            || cwd.len() > MAX_CWD_BYTES
            || !workspace.valid()
        {
            return false;
        }
        self.workspaces.push(workspace);
        self.save();
        true
    }

    /// Renames a kept workspace; whether its name changed.
    pub(crate) fn rename_kept(&mut self, scope: &Scope, id: &str, label: &str) -> bool {
        let label = clean_label(label);
        let Some(workspace) = self
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.scope == scope.as_str() && workspace.id == id)
        else {
            return false;
        };
        if label.is_empty() || (workspace.custom && workspace.label == label) {
            return false;
        }
        workspace.label = label;
        workspace.custom = true;
        self.save();
        true
    }

    /// Forgets a kept workspace and hands back the tabs it held, whose
    /// terminals the caller ends.
    pub(crate) fn forget_kept(&mut self, scope: &Scope, id: &str) -> Vec<Tab> {
        let before = self.workspaces.len();
        self.workspaces
            .retain(|workspace| workspace.scope != scope.as_str() || workspace.id != id);
        let tabs: Vec<Tab> = self
            .tabs
            .iter()
            .filter(|tab| &tab.scope == scope && tab.workspace_id == id)
            .cloned()
            .collect();
        let closed = [id.to_owned()];
        if !self.forget_workspaces(scope, &closed) && self.workspaces.len() != before {
            self.save();
        }
        tabs
    }

    /// Moves a kept workspace beside another of its host; whether it moved.
    pub(crate) fn move_kept(&mut self, scope: &Scope, id: &str, beside: Beside<&str>) -> bool {
        let (Beside::Before(anchor) | Beside::After(anchor)) = beside;
        let find = |workspaces: &[SavedWorkspace], id: &str| {
            workspaces
                .iter()
                .position(|workspace| workspace.scope == scope.as_str() && workspace.id == id)
        };
        if anchor == id {
            return false;
        }
        let (Some(from), Some(_)) = (find(&self.workspaces, id), find(&self.workspaces, anchor))
        else {
            return false;
        };
        let workspace = self.workspaces.remove(from);
        let Some(at) = find(&self.workspaces, anchor) else {
            self.workspaces.insert(from, workspace);
            return false;
        };
        let to = match beside {
            Beside::Before(_) => at,
            Beside::After(_) => at + 1,
        };
        self.workspaces.insert(to, workspace);
        if to == from {
            return false;
        }
        self.save();
        true
    }
}

/// Whether `id` names a workspace the app keeps rather than Herdr 1's.
pub(crate) fn is_kept_id(id: &str) -> bool {
    id == crate::core_terminals::WORKSPACE || id.starts_with(PREFIX)
}

/// A name the user typed, on one line and within bounds.
fn clean_label(label: &str) -> String {
    crate::notifications::safe_text(label, MAX_LABEL_CHARS * 4)
        .chars()
        .take(MAX_LABEL_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}
