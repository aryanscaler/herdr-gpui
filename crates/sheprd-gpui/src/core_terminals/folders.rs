//! The folders views take from herdr-core panes, as they take a Herdr
//! pane's: code search, the editor, and an editor tab's starting folder ask
//! for the folder the user works in, whichever backend runs its terminal.

use super::CoreTerminals;
use crate::{SheprdWindow, browser::Scope};
use gpui::App;
use std::path::PathBuf;

impl CoreTerminals {
    /// The folder a workspace's herdr-core tabs started in, as Herdr names
    /// a workspace's by its first tab's first pane: where the first pane of
    /// the oldest tab that drives its terminals was first seen, so moving
    /// around in that pane does not move the workspace.
    pub(crate) fn launch_root(&self, scope: &Scope, workspace: &str) -> Option<PathBuf> {
        let (_, tab) = self
            .tabs
            .iter()
            .filter(|(_, shown)| !shown.observe)
            .filter(|(_, shown)| {
                shown
                    .workspace
                    .as_ref()
                    .is_some_and(|(known, id)| known == scope && id == workspace)
            })
            .min_by_key(|(id, _)| **id)?;
        tab.root.clone()
    }
}

impl super::TabPanes {
    /// Keeps the first folder herdr-core reports for the tab's first pane.
    pub(super) fn keep_root(&mut self, infos: &super::infos::Infos) {
        if self.root.is_none() {
            self.root = infos
                .get(self.panes.first())
                .and_then(|info| info.cwd.clone());
        }
    }
}

impl CoreTerminals {
    /// The scope and workspace of `tab`, once the window has shown it.
    pub(crate) fn tab_workspace(&self, tab: crate::browser::TabId) -> Option<&(Scope, String)> {
        self.tabs.get(&tab)?.workspace.as_ref()
    }

    /// Each terminal the window shows, by its core and its address there,
    /// and the workspace it is in, for what names a terminal but not its
    /// workspace: the terminals of its tabs, and the shell shown in place of
    /// Herdr 1, in the workspace that stands in for Herdr's. A saved host's
    /// terminals are named with that host, as the same address there is
    /// another terminal than here.
    ///
    /// Each workspace is named with the daemon whose tabs hold it, and the
    /// shell's with `fallback`, the daemon of the host the window shows.
    pub(crate) fn terminal_workspaces(
        &self,
        fallback: Option<Scope>,
    ) -> Vec<(super::Host, String, crate::listening_ports::Place)> {
        let fallback = self
            .fallback_terminal()
            .zip(fallback)
            .map(|(terminal, scope)| {
                (
                    super::Host::Local,
                    terminal.as_str().to_owned(),
                    (scope, super::WORKSPACE.to_owned()),
                )
            });
        // A terminal opened to watch in another workspace stays in the one
        // whose tab drives it: driving tabs come first, oldest first, and the
        // port lookups keep a terminal's first workspace.
        let mut tabs: Vec<_> = self
            .tabs
            .iter()
            .filter_map(|(id, shown)| Some((shown.observe, *id, shown, shown.workspace.as_ref()?)))
            .collect();
        tabs.sort_unstable_by_key(|(observe, id, ..)| (*observe, *id));
        let mut places = Vec::new();
        let mut fallback = fallback;
        for (observe, _, shown, place) in tabs {
            if observe && let Some(shell) = fallback.take() {
                places.push(shell);
            }
            for terminal in shown.panes.terminals() {
                places.push((
                    shown.host.clone(),
                    terminal.as_str().to_owned(),
                    place.clone(),
                ));
            }
        }
        places.extend(fallback);
        places
    }
}

impl SheprdWindow {
    /// The folder of the pane the user works in: the herdr-core pane the
    /// active group shows, else Herdr's focused pane, where its program is
    /// rather than where its shell started.
    pub(crate) fn focused_folder(&self, cx: &App) -> Option<String> {
        if let Some((tab, terminal)) = self.shown_core_pane(cx) {
            return self
                .core_folder(tab, &terminal)
                .and_then(|folder| folder.into_os_string().into_string().ok());
        }
        let snapshot = self.live.snapshot.as_deref()?;
        let id = snapshot.focused_pane_id.as_deref()?;
        let pane = snapshot.panes.iter().find(|pane| pane.pane_id == id)?;
        pane.foreground_cwd.clone().or_else(|| pane.cwd.clone())
    }

    /// The checkout of the herdr-core pane the active group showed at the
    /// last tick, once Git named its branch. `None` when no herdr-core tab
    /// is shown, or its pane is in no checkout or on a detached head.
    pub(crate) fn core_git_input(&self) -> Option<crate::pull_request::Input> {
        let checkout = self.shown_core_folder()?;
        let branch = self
            .core_terminals
            .checkouts
            .get(&checkout)?
            .branch
            .clone()?;
        Some(crate::pull_request::Input {
            checkout: Some(checkout),
            repo_key: None,
            branch,
        })
    }

    /// The folder of the herdr-core pane the active group showed at the
    /// last tick.
    pub(super) fn shown_core_folder(&self) -> Option<String> {
        let (tab, terminal) = self.core_terminals.shown.as_ref()?;
        self.core_folder(*tab, terminal)?
            .into_os_string()
            .into_string()
            .ok()
    }

    /// Whether the active group showed a herdr-core tab at the last tick.
    pub(crate) fn shows_core_tab(&self) -> bool {
        self.core_terminals.shown.is_some()
    }

    /// The folder the focused workspace's herdr-core tabs started in, for a
    /// workspace with no Herdr pane to name one.
    pub(crate) fn core_launch_root(&self) -> Option<String> {
        let (scope, workspace) = self.browser_key()?;
        self.core_terminals
            .launch_root(&scope, &workspace)?
            .into_os_string()
            .into_string()
            .ok()
    }
}
