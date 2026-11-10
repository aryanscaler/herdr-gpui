//! The workspaces the app keeps for Herdr 2 terminals: which one is in
//! front, and creating, renaming, closing, and moving them. herdr-core has
//! no workspaces, so these are the app's own, saved with its tabs
//! (`browser::Store`) under the scope of the host whose core runs their
//! terminals, and shown beside Herdr 1's: on this computer, or on a saved
//! SSH, cloud, or WSL host.

use super::{Host, Purpose, WORKSPACE};
use crate::{
    SheprdWindow,
    browser::{Location, Scope, Store, scope},
    i18n::fl,
    reorder::Beside,
    window::Flash,
};
use gpui::{prelude::*, *};
use sheprd_backend::Spawn;
use std::path::{Path, PathBuf};

impl SheprdWindow {
    /// The kept workspace in front of the selected host, if one is: the one
    /// the user last chose, or with no Herdr 1 installed, the first.
    pub(crate) fn kept_in_front(&self) -> Option<&str> {
        let scope = scope_of(self);
        let session = self.core_terminals.session_of(&scope)?;
        let chosen = self
            .core_terminals
            .in_front
            .as_ref()
            .filter(|(chosen, _)| *chosen == scope)
            .and_then(|(_, id)| session.workspace(id));
        let chosen = match chosen {
            Some(workspace) => Some(workspace),
            None if self.live.herdr_1_unavailable() => session.workspaces.first(),
            None => None,
        };
        chosen.map(|workspace| workspace.id.as_str())
    }

    /// Whether `id` is a workspace the app keeps on the selected host.
    pub(crate) fn is_kept_workspace(&self, id: &str) -> bool {
        self.core_terminals
            .session_of(&scope_of(self))
            .is_some_and(|session| session.workspace(id).is_some())
    }

    /// Brings a kept workspace to the front; whether it is one.
    pub(crate) fn focus_kept_workspace(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        if !self.is_kept_workspace(id) {
            return false;
        }
        self.core_terminals.in_front = Some((scope_of(self), id.to_owned()));
        self.core_terminals.reveal = true;
        cx.notify();
        true
    }

    /// Hands the front back to Herdr 1's focused workspace, as when one of
    /// its workspaces is chosen.
    pub(crate) fn leave_kept_workspace(&mut self) {
        self.core_terminals.in_front = None;
    }

    /// Keeps a new workspace on the selected host, brings it to the front,
    /// and starts a shell in a tab of it, in `cwd` when given: a folder on
    /// that host.
    pub(crate) fn new_kept_workspace(
        &mut self,
        label: Option<&str>,
        cwd: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.keep_new_workspace(label, cwd.as_deref(), cx) else {
            return;
        };
        let spawn = Spawn {
            cwd,
            ..Spawn::default()
        };
        self.start_core_program(
            Purpose::Tab {
                scope: scope_of(self),
                workspace: id,
            },
            spawn,
            cx,
        );
    }

    /// Keeps a new workspace for a checkout at `cwd` whose first tab shows
    /// `location`, such as a herdr-core terminal a worker already started,
    /// and brings it to the front. Answers its ID, or `None` when it could
    /// not be kept or its tab not opened, which a flash says. Then nothing
    /// is left behind: the workspace is not kept, and the terminals
    /// `location` drives end, since no tab would show them.
    // Callers come with opening worktrees and dispatching agents to Herdr 2.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn open_kept_workspace(
        &mut self,
        label: Option<&str>,
        cwd: &Path,
        location: Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let scope = scope_of(self);
        let host = Host::of(&scope);
        let Some(id) = self.keep_new_workspace(label, Some(cwd), cx) else {
            self.end_location(&host, &location);
            return None;
        };
        let shown = location.clone();
        let tab = Store::update(cx, |store| store.open(scope, &id, Some(shown), None));
        let Some(tab) = tab else {
            self.close_kept_workspace(&id, window, cx);
            self.end_location(&host, &location);
            self.show_flash(Flash::warning(fl!("core-terminals-too-many-tabs")), cx);
            return None;
        };
        self.sync_core_views(cx);
        self.show_browser_tab(tab, window, cx);
        Some(id)
    }

    /// Ends the herdr-core terminals `location` drives on `host`'s core; a
    /// mirror only watches.
    fn end_location(&self, host: &Host, location: &Location) {
        if let Location::CoreTerminal {
            panes,
            observe: false,
            ..
        } = location
        {
            for terminal in panes.terminals() {
                self.end_core_terminal(host, terminal.clone());
            }
        }
    }

    /// Whether a new workspace on this computer, such as a project's,
    /// is one the app keeps for Herdr 2, whichever host is shown.
    pub(crate) fn local_workspaces_use_core(&self) -> bool {
        let Some(index) = self.core_host() else {
            return false;
        };
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &self.endpoints[index].live
        };
        self.config.herdr_2_terminals || live.missing_installation
    }

    /// Shows this computer and brings its kept workspace in `cwd` to the
    /// front, keeping a new one named `label` when there is none.
    pub(crate) fn open_local_kept_folder(
        &mut self,
        label: &str,
        cwd: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if !self.show_this_computer(cx) {
            return;
        }
        match self.kept_workspace_at(&cwd) {
            Some(id) => {
                self.focus_kept_workspace(&id, cx);
            }
            None => self.new_kept_workspace(Some(label), Some(cwd), cx),
        }
    }

    /// Shows this computer, whose workspaces the app keeps, when another
    /// host is shown; whether it is shown now.
    pub(crate) fn show_this_computer(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.selected_is_remote() {
            return true;
        }
        let Some(index) = self.core_host() else {
            return false;
        };
        let local = self.endpoints[index].id.clone();
        if !self.select_endpoint(&local, cx) {
            return false;
        }
        self.refresh_core_session(cx);
        true
    }

    /// The kept workspace on the selected host whose folder is `cwd`.
    pub(crate) fn kept_workspace_at(&self, cwd: &Path) -> Option<String> {
        self.core_terminals
            .session_of(&scope_of(self))?
            .workspaces
            .iter()
            .find(|workspace| !workspace.cwd.is_empty() && Path::new(&workspace.cwd) == cwd)
            .map(|workspace| workspace.id.clone())
    }

    /// This computer's scope, whose workspaces the app keeps, whichever
    /// host is shown.
    pub(crate) fn local_kept_scope(&self) -> Option<Scope> {
        self.core_host().map(|index| scope(&self.endpoints[index]))
    }

    /// The workspace kept for `scope` whose folder is `cwd`, whichever host
    /// is shown.
    pub(crate) fn kept_workspace_in(&self, scope: &Scope, cwd: &Path, cx: &App) -> Option<String> {
        cx.try_global::<Store>()?
            .kept_workspaces(scope)
            .find(|workspace| !workspace.cwd.is_empty() && Path::new(&workspace.cwd) == cwd)
            .map(|workspace| workspace.id.clone())
    }

    /// Closes the workspace this computer keeps at `cwd`, without showing
    /// this computer; whether there was one.
    pub(crate) fn close_local_kept_folder(
        &mut self,
        cwd: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(scope) = self.local_kept_scope() else {
            return false;
        };
        let Some(id) = self.kept_workspace_in(&scope, cwd, cx) else {
            return false;
        };
        self.close_kept_workspace_in(&scope, &id, window, cx);
        true
    }

    /// Keeps a new workspace on this computer for a checkout at `cwd` whose
    /// first tab shows `location`, leaving whatever is in front, and which
    /// host is shown, as they are, as Herdr 1 opens a fan-out lane's
    /// workspace without focusing it. Answers its ID, or `None`, with a
    /// flash, when it could not be kept or its tab not opened, which ends
    /// the terminals `location` drives.
    pub(crate) fn open_kept_workspace_behind(
        &mut self,
        label: &str,
        cwd: &Path,
        location: Location,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let kept = self.local_kept_scope().and_then(|scope| {
            Some((
                self.keep_workspace_in(&scope, Some(label), Some(cwd), cx)?,
                scope,
            ))
        });
        let Some((id, scope)) = kept else {
            self.end_location(&Host::Local, &location);
            return None;
        };
        let shown = location.clone();
        let tab = Store::update(cx, |store| {
            store.open(scope.clone(), &id, Some(shown), None)
        });
        if tab.is_none() {
            Store::update(cx, |store| store.forget_kept(&scope, &id));
            self.refresh_core_session(cx);
            self.end_location(&Host::Local, &location);
            self.show_flash(Flash::warning(fl!("core-terminals-too-many-tabs")), cx);
            return None;
        }
        self.sync_core_views(cx);
        cx.notify();
        Some(id)
    }

    /// Keeps a new workspace and brings it to the front, named for `label`,
    /// else its folder.
    pub(super) fn keep_new_workspace(
        &mut self,
        label: Option<&str>,
        cwd: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let id = self.keep_workspace(label, cwd, cx)?;
        self.focus_kept_workspace(&id, cx);
        Some(id)
    }

    /// Keeps a new workspace on the selected host, named for `label`, else
    /// its folder, without bringing it to the front.
    fn keep_workspace(
        &mut self,
        label: Option<&str>,
        cwd: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.core_terminals.lacks_core(&Host::of(&scope_of(self))) {
            self.show_flash(
                Flash::warning(fl!("core-terminals-workspaces-need-core")),
                cx,
            );
            return None;
        }
        self.keep_workspace_in(&scope_of(self), label, cwd, cx)
    }

    /// Keeps a new workspace for `scope`, named for `label`, else its folder.
    fn keep_workspace_in(
        &mut self,
        scope: &Scope,
        label: Option<&str>,
        cwd: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let folder = cwd.map(|cwd| cwd.display().to_string());
        let (label, custom) = kept_label(label, cwd);
        let kept = Store::update(cx, |store| {
            store.keep_workspace(scope, &label, folder.as_deref().unwrap_or(""), custom)
        });
        let Some(id) = kept else {
            self.show_flash(Flash::warning(fl!("core-workspaces-too-many")), cx);
            return None;
        };
        self.refresh_core_session(cx);
        Some(id)
    }

    /// The folder of the workspace in front, which a new workspace starts in.
    pub(crate) fn front_workspace_cwd(&self) -> Option<PathBuf> {
        let (_, front) = self.browser_key()?;
        let sessions = self.sessions(self.selected_endpoint);
        let cwd = &sessions.workspace(&front)?.cwd;
        (!cwd.is_empty()).then(|| PathBuf::from(cwd))
    }

    /// Renames a kept workspace on the selected host.
    pub(crate) fn rename_kept_workspace(&mut self, id: &str, label: &str, cx: &mut Context<Self>) {
        let scope = scope_of(self);
        Store::update(cx, |store| store.rename_kept(&scope, id, label));
        self.refresh_core_session(cx);
        cx.notify();
    }

    /// Closes a kept workspace: its tabs close and the terminals they drive
    /// end, as closing a Herdr 1 workspace ends its panes.
    pub(crate) fn close_kept_workspace(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_kept_workspace_in(&scope_of(self), id, window, cx);
    }

    /// [`Self::close_kept_workspace`] for a workspace kept for `scope`, such
    /// as one whose checkout was removed while another host was shown.
    pub(crate) fn close_kept_workspace_in(
        &mut self,
        scope: &Scope,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let host = Host::of(scope);
        let tabs = Store::update(cx, |store| store.forget_kept(scope, id));
        let closed: Vec<String> = tabs.iter().map(|tab| tab.id.to_string()).collect();
        crate::notification_mutes::Mutes::update(cx, |mutes| {
            mutes.forget_kept(scope.as_str(), id, &closed)
        });
        for location in tabs.iter().filter_map(|tab| tab.location.as_ref()) {
            self.end_location(&host, location);
        }
        self.forget_browser_tabs(|gone| tabs.iter().any(|tab| tab.id == gone));
        if self
            .core_terminals
            .in_front
            .as_ref()
            .is_some_and(|(_, front)| front == id)
        {
            self.core_terminals.in_front = None;
            self.core_terminals.reveal = true;
        }
        self.refresh_core_session(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Closes the kept workspace `id` of `scope` when it has no tab left,
    /// as when the first terminal it was made for could not start.
    pub(super) fn forget_empty_kept_workspace(
        &mut self,
        scope: &Scope,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let empty = cx.try_global::<Store>().is_some_and(|store| {
            store.kept_workspace(scope, id).is_some()
                && store.in_workspace(scope, id).next().is_none()
        });
        if !empty {
            return;
        }
        if *scope == scope_of(self) {
            self.close_kept_workspace(id, window, cx);
            return;
        }
        Store::update(cx, |store| store.forget_kept(scope, id));
        self.refresh_core_session(cx);
    }

    /// Moves a kept workspace beside another of the selected host's.
    pub(crate) fn move_kept_workspace(
        &mut self,
        id: &str,
        beside: Beside<&str>,
        cx: &mut Context<Self>,
    ) -> bool {
        let scope = scope_of(self);
        let moved = Store::update(cx, |store| store.move_kept(&scope, id, beside));
        if moved {
            self.refresh_core_session(cx);
            cx.notify();
        }
        moved
    }

    /// Whether `workspace` was a kept workspace of `scope` that has closed:
    /// a kept ID the store no longer holds.
    pub(super) fn kept_workspace_gone(&self, scope: &Scope, workspace: &str, cx: &App) -> bool {
        crate::browser::is_kept_id(workspace)
            && cx
                .try_global::<Store>()
                .is_none_or(|store| store.kept_workspace(scope, workspace).is_none())
    }

    /// Shows a tab of the kept workspace just brought to the front, so its
    /// groups do not keep drawing Herdr 1's terminal, and gives a terminal
    /// shown there the keyboard.
    pub(super) fn reveal_kept_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.core_terminals.reveal) {
            return;
        }
        let Some((scope, workspace)) = self.browser_key() else {
            return;
        };
        if !self.is_kept_workspace(&workspace) {
            return;
        }
        let shown = self
            .active_group()
            .and_then(|group| self.group_pick(group))
            .and_then(|pick| match pick {
                crate::browser::Pick::Page(id) => Some(id),
                _ => None,
            });
        if let Some(tab) = shown {
            let terminal = cx
                .try_global::<Store>()
                .and_then(|store| store.get(tab))
                .is_some_and(|tab| matches!(tab.location, Some(Location::CoreTerminal { .. })));
            if terminal {
                self.focus_core_terminal(tab, window, cx);
            }
            return;
        }
        let first = cx
            .try_global::<Store>()
            .and_then(|store| store.in_workspace(&scope, &workspace).next())
            .map(|tab| tab.id);
        if let Some(tab) = first {
            self.show_browser_tab(tab, window, cx);
        }
    }

    /// Whether the selected host still keeps the workspace a window with
    /// no Herdr 1 installed starts with, whose shell stands in for Herdr 1's.
    pub(super) fn default_workspace_kept(&self, cx: &App) -> bool {
        cx.try_global::<Store>()
            .is_none_or(|store| store.kept_workspace(&scope_of(self), WORKSPACE).is_some())
    }

    /// Whether the workspace the stand-in shell belongs to is the one the
    /// window shows, which a window with no workspace to show always is.
    pub(super) fn default_workspace_shown(&self) -> bool {
        self.browser_key()
            .is_none_or(|(scope, workspace)| scope == scope_of(self) && workspace == WORKSPACE)
    }

    /// Keeps the workspace a window with no Herdr 1 installed gathers its
    /// tabs in, so it lists, renames, and closes like any other. Whether it
    /// kept it now: never once the store is full.
    pub(super) fn keep_default_workspace(
        &self,
        missing_installation: bool,
        scope: &Scope,
        cx: &mut App,
    ) -> bool {
        if !missing_installation
            || cx
                .try_global::<Store>()
                .is_some_and(|store| store.kept_workspaces(scope).next().is_some())
        {
            return false;
        }
        let label = fl!("core-workspaces-default");
        Store::update(cx, |store| store.keep_default(scope, WORKSPACE, &label))
    }
}

/// The name a new kept workspace in `cwd` gets: `label`, else its folder's,
/// and whether it is the user's. A name given that is not just its folder's
/// is, which a worktree row keeps rather than showing the branch.
pub(crate) fn kept_label(label: Option<&str>, cwd: Option<&Path>) -> (String, bool) {
    let named = cwd
        .and_then(|cwd| cwd.file_name())
        .map(|name| name.to_string_lossy().into_owned());
    let custom = label.is_some_and(|label| Some(label) != named.as_deref());
    let label = label
        .map(str::to_owned)
        .or(named)
        .unwrap_or_else(|| fl!("core-workspaces-default"));
    (label, custom)
}

pub(super) fn scope_of(window: &SheprdWindow) -> Scope {
    scope(&window.endpoints[window.selected_endpoint])
}
