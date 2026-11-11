//! What a host's layout asks of the window, and how the window does it: each
//! goes through the code the window's own controls call, so a host's row,
//! menu, or fold behaves as the window's own.

use super::{super::ActivityPanel, panel_id};
use crate::{controls::Command, navigation::NavigationTarget, window::SheprdWindow};
use gpui::{Context, Window};

/// What a host's layout asks of the window. A device is named by its
/// endpoint id; none means the selected one.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Runs a command by its name in the catalog, as a shortcut does.
    Command(String),
    /// Opens or closes the note on a workspace's checkout, at the pointer.
    EditNote {
        endpoint: Option<String>,
        workspace: String,
    },
    /// Folds every repository's linked worktrees on every listed device, or
    /// unfolds them all.
    FoldAll(bool),
    /// Focuses the pane with this id.
    FocusPane {
        endpoint: Option<String>,
        pane: String,
    },
    /// Focuses the workspace with this id.
    FocusWorkspace {
        endpoint: Option<String>,
        workspace: String,
    },
    /// Opens the window's own menu at the pointer, as a right click on its
    /// row does.
    Menu(Menu),
    /// Opens the pull request the build's worktree is for.
    OpenBuildPullRequest,
    /// Moves a workspace of the selected device before `target`, or after it
    /// with `after`, as dropping its row there does.
    MoveWorkspace {
        workspace: String,
        target: String,
        after: bool,
    },
    /// Shows the activity panel with this id, as its button does.
    Panel(String),
    /// Selects a device, as a click on its header does.
    SelectDevice(String),
    /// Resizes the sidebar as its edge is dragged: `dx` pixels from where the
    /// press was. A double press puts back the default width.
    SidebarWidth { phase: Drag, dx: f32 },
    /// Switches the agents list between grouped and by priority.
    ToggleAgentSort,
    /// Folds or unfolds a device's header.
    ToggleDevice(String),
    /// Folds or unfolds a repository's linked worktrees.
    ToggleRepository {
        endpoint: Option<String>,
        key: String,
    },
}

/// Where a drag of the sidebar's edge is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drag {
    /// The press; `reset` on a double press.
    Down {
        reset: bool,
    },
    Move,
    Up,
}

thread_local! {
    /// The sidebar's width when its edge was pressed, which a drag adds to.
    static WIDTH_AT_PRESS: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// The window's menus a host's row opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Menu {
    /// An agent's pane menu.
    Agent {
        endpoint: Option<String>,
        pane: String,
    },
    /// A device's menu.
    Device(String),
    /// A workspace's popover: its details, actions, and pull request.
    Workspace {
        endpoint: Option<String>,
        workspace: String,
    },
}

impl SheprdWindow {
    /// The endpoint id `endpoint` names, the selected one when none.
    fn hosted_endpoint(&self, endpoint: Option<String>) -> Option<String> {
        endpoint.or_else(|| {
            self.endpoints
                .get(self.selected_endpoint)
                .map(|endpoint| endpoint.id.clone())
        })
    }

    /// Does what a host's layout asked. An id no longer shown is ignored: it
    /// came from a state a moment old.
    pub(crate) fn hosted_action(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pointer = window.mouse_position();
        match action {
            Action::Command(name) => {
                let found = sheprd_commands::COMMANDS
                    .iter()
                    .find(|info| info.name == name);
                if let Some(info) = found {
                    let command: Command = info.command;
                    self.command(command, window, cx);
                }
            }
            Action::EditNote {
                endpoint,
                workspace,
            } => {
                if let Some(endpoint) = self.hosted_endpoint(endpoint) {
                    self.edit_worktree_note(&endpoint, &workspace, pointer, window, cx);
                }
            }
            Action::FoldAll(fold) => self.fold_all_repositories(fold, cx),
            Action::FocusPane { endpoint, pane } => {
                if let Some(endpoint) = self.hosted_endpoint(endpoint) {
                    self.focus_listed_agent(&endpoint, &pane, window, cx);
                }
            }
            Action::FocusWorkspace {
                endpoint,
                workspace,
            } => {
                if let Some(endpoint) = self.hosted_endpoint(endpoint) {
                    self.navigate_endpoint(&endpoint, NavigationTarget::Workspace(&workspace), cx);
                }
            }
            Action::Menu(menu) => self.hosted_menu(menu, pointer, window, cx),
            Action::MoveWorkspace {
                workspace,
                target,
                after,
            } => self.move_workspace_to(&workspace, &target, after, cx),
            Action::OpenBuildPullRequest => {
                let pr = env!("HERDR_BUILD_PR");
                if !pr.is_empty() {
                    cx.open_url(&crate::worktree_banner::pull_request_url(pr));
                }
            }
            Action::Panel(id) => {
                let panel = ActivityPanel::ALL
                    .iter()
                    .copied()
                    .find(|panel| panel_id(*panel) == id);
                if let Some(panel) = panel {
                    self.select_activity_panel(panel, window, cx);
                }
            }
            Action::SelectDevice(id) => {
                self.select_endpoint(&id, cx);
                window.focus(&self.focus, cx);
            }
            Action::SidebarWidth { phase, dx } => self.drag_sidebar_edge(phase, dx, window, cx),
            Action::ToggleAgentSort => {
                self.agent_sort = self.agent_sort.toggled();
                self.agent_sort_modified = true;
                self.save_chrome();
                cx.notify();
            }
            Action::ToggleDevice(id) => self.toggle_device(&id, cx),
            Action::ToggleRepository { endpoint, key } => {
                if let Some(endpoint) = self.hosted_endpoint(endpoint) {
                    self.toggle_repository(&endpoint, &key, cx);
                }
            }
        }
    }

    /// Shows the agent the agents list lists as `id` on `endpoint`, as a click
    /// on its row does: a Herdr 1 pane, or a herdr-core terminal in its tab.
    /// An id no longer listed is still tried as a Herdr 1 pane.
    fn focus_listed_agent(
        &mut self,
        endpoint: &str,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .panel_agents()
            .into_iter()
            .find(|(index, agent)| {
                agent.id() == id
                    && self
                        .endpoints
                        .get(*index)
                        .is_some_and(|listed| listed.id == endpoint)
            })
            .map(|(_, agent)| agent.target());
        match target {
            Some(target) => self.open_listed_agent(endpoint, &target, window, cx),
            None => {
                self.navigate_endpoint(endpoint, NavigationTarget::Pane(id), cx);
            }
        }
    }

    /// Opens `menu` at `anchor`, after showing what it is for, as a right
    /// click on the window's own row does.
    fn hosted_menu(
        &mut self,
        menu: Menu,
        anchor: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match menu {
            Menu::Agent { endpoint, pane } => {
                if let Some(endpoint) = self.hosted_endpoint(endpoint) {
                    self.open_agent_menu(&endpoint, &pane, anchor, window, cx);
                }
            }
            Menu::Device(id) => self.open_host_menu(&id, anchor, window, cx),
            Menu::Workspace {
                endpoint,
                workspace,
            } => {
                let Some(endpoint) = self.hosted_endpoint(endpoint) else {
                    return;
                };
                // The sheet opens over the row without selecting it; a row on
                // another device only switches the device first, as the sheet
                // acts on the selected one.
                let on_selected = self
                    .endpoints
                    .get(self.selected_endpoint)
                    .is_some_and(|selected| selected.id == endpoint);
                if !on_selected && !self.select_endpoint(&endpoint, cx) {
                    return;
                }
                self.open_workspace_menu(&workspace, anchor, window, cx);
                self.menu.opening_right_click =
                    self.menu.page == Some(crate::menu::Page::Workspace);
            }
        }
    }

    /// Resizes the sidebar from a host's drag of its edge, as dragging the
    /// window's own edge does, and keeps the width when the drag ends.
    fn drag_sidebar_edge(&mut self, phase: Drag, dx: f32, window: &Window, cx: &mut Context<Self>) {
        let viewport = f32::from(window.viewport_size().width);
        match phase {
            Drag::Down { reset: true } => {
                WIDTH_AT_PRESS.set(None);
                self.sidebar_modified = true;
                self.sidebar_width = None;
                self.save_sidebar_width();
            }
            Drag::Down { reset: false } => {
                let width = self.sidebar_mode().width(self.sidebar_width, viewport);
                WIDTH_AT_PRESS.set(width);
                self.sidebar_modified = true;
            }
            Drag::Move => {
                let Some(start) = WIDTH_AT_PRESS.get() else {
                    return;
                };
                let toward_end = crate::direction::toward_end(dx);
                self.sidebar_width = Some(super::super::metrics::sidebar_width(
                    Some(start + toward_end),
                    viewport,
                ));
            }
            Drag::Up => {
                if WIDTH_AT_PRESS.take().is_some() {
                    self.save_sidebar_width();
                }
            }
        }
        cx.notify();
    }

    /// Folds every grouped repository on every device, or unfolds them all.
    fn fold_all_repositories(&mut self, fold: bool, cx: &mut Context<Self>) {
        let groups: Vec<(usize, Vec<String>)> = (0..self.endpoints.len())
            .map(|index| {
                let sessions = self.sessions(index);
                let keys = sessions
                    .each()
                    .flat_map(|session| {
                        super::super::workspaces::grouped_keys(&session.workspaces)
                            .into_iter()
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .collect();
                (index, keys)
            })
            .collect();
        for (index, keys) in groups {
            let collapsed = if index == 0 {
                &mut self.collapsed_repos
            } else {
                &mut self.endpoints[index].collapsed_repos
            };
            if fold {
                collapsed.extend(keys);
            } else {
                collapsed.clear();
            }
        }
        cx.notify();
    }
}
