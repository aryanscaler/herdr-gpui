//! What a terminal the app starts in herdr-core takes from where it opens:
//! the folder it starts in and the variables that tie it to its workspace.
//!
//! herdr-core starts a program in its own server's folder with the server's
//! environment. A core started from a Herdr pane carries that pane's
//! `HERDR_PANE_ID`, so a program in any of its terminals would report to,
//! and be targeted as, a pane it is not in. Every spawn therefore names its
//! Herdr identity outright: the workspace and daemon of a Herdr workspace it
//! opens in, and no pane or tab, which herdr-core terminals are not.
//!
//! On a saved host a terminal starts in that host's folders but names no
//! Herdr workspace: its daemon's API socket lies under that host's home,
//! which this computer does not know, and a spawn cannot clear an inherited
//! `HERDR_SOCKET_PATH`, which would win over a session named beside it. A
//! workspace the app keeps names none either: its terminals are found by
//! `HC_TERMINAL_ID` and the tab that holds them.

use super::{Host, Purpose};
use crate::{SheprdWindow, browser::Shown};
use gpui::App;
use sheprd_backend::{Spawn, TerminalId};
use std::path::{Path, PathBuf};

const PANE: &str = "HERDR_PANE_ID";
const SOCKET: &str = "HERDR_SOCKET_PATH";
const TAB: &str = "HERDR_TAB_ID";
const WORKSPACE: &str = "HERDR_WORKSPACE_ID";

/// The Herdr workspace a terminal opens in, as Herdr names it in its panes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Herdr {
    /// The daemon's API socket, as `HERDR_SOCKET_PATH`.
    pub api_socket: PathBuf,
    pub workspace: String,
}

/// Gives `spawn` the folder and variables of where it opens. What the spawn
/// already names wins, such as a chat's own `HERDR_PANE_ID`.
pub(super) fn apply(spawn: &mut Spawn, folder: Option<PathBuf>, herdr: Option<Herdr>) {
    if spawn.cwd.is_none() {
        spawn.cwd = folder;
    }
    let mut set = |name: &str, value: String| {
        if !spawn.env.iter().any(|(known, _)| known == name) {
            spawn.env.push((name.to_owned(), value));
        }
    };
    set(PANE, String::new());
    set(TAB, String::new());
    match herdr {
        Some(Herdr {
            api_socket,
            workspace,
        }) => {
            set(SOCKET, api_socket.to_string_lossy().into_owned());
            set(WORKSPACE, workspace);
        }
        // An inherited socket is harmless without a workspace to go with it,
        // and an empty one would read as the default daemon's.
        None => set(WORKSPACE, String::new()),
    }
}

/// The API socket a daemon serves beside the client socket `client`:
/// `herdr.sock` beside `herdr-client.sock`, the reverse of what discovery
/// derives. A socket not named that way has none we can tell.
pub(super) fn api_socket(client: &Path) -> Option<PathBuf> {
    let stem = client.file_stem()?.to_str()?.strip_suffix("-client")?;
    if stem.is_empty() {
        return None;
    }
    let extension = client.extension().map_or_else(String::new, |extension| {
        format!(".{}", extension.to_string_lossy())
    });
    Some(client.with_file_name(format!("{stem}{extension}")))
}

impl SheprdWindow {
    /// Gives `spawn` the folder and Herdr variables of where `purpose` opens
    /// it: a split starts where the pane it splits is, a tab in its
    /// workspace's folder. A saved host's core knows none of this
    /// computer's folders or daemons, so there the folders are that host's
    /// own and no Herdr workspace is named.
    pub(super) fn give_core_context(
        &self,
        purpose: &Purpose,
        host: &Host,
        spawn: &mut Spawn,
        cx: &App,
    ) {
        if let Host::Remote(scope) = host {
            let folder = match purpose {
                Purpose::Split { tab, at, .. } => self
                    .core_terminals
                    .info(*tab, at)
                    .and_then(|info| info.cwd.clone()),
                Purpose::Tab { workspace, .. } => {
                    self.remote_workspace_folder(scope, workspace, cx)
                }
            };
            apply(spawn, folder, None);
            return;
        }
        let workspace = match purpose {
            Purpose::Tab { workspace, .. } => Some(workspace.clone()),
            Purpose::Split { tab, .. } => cx
                .try_global::<crate::browser::Store>()
                .and_then(|store| store.get(*tab))
                .map(|tab| tab.workspace_id.clone()),
        };
        let beside = match purpose {
            Purpose::Split { tab, at, .. } => self.core_folder(*tab, at),
            Purpose::Tab { .. } => None,
        };
        let folder = beside
            .or_else(|| {
                workspace
                    .as_deref()
                    .and_then(|workspace| self.core_workspace_folder(workspace, cx))
            })
            .or_else(sheprd_client::local_path::home_dir);
        let herdr = workspace.and_then(|workspace| self.herdr_workspace(workspace));
        apply(spawn, folder, herdr);
    }

    /// The folder `terminal` of `tab` is in now, as this computer's
    /// herdr-core reports it. A saved host's folder is not on this
    /// computer, so none is named for its terminals.
    pub(crate) fn core_folder(
        &self,
        tab: crate::browser::TabId,
        terminal: &TerminalId,
    ) -> Option<PathBuf> {
        if self.core_terminals.host_of(tab) != Host::Local {
            return None;
        }
        self.core_terminals.info(tab, terminal)?.cwd.clone()
    }

    /// The herdr-core tab the active group shows, and its pane in use.
    pub(crate) fn shown_core_pane(&self, cx: &App) -> Option<(crate::browser::TabId, TerminalId)> {
        let Shown::Page(tab) = self.group_shown(self.active_group()?, cx) else {
            return None;
        };
        let shown = self.core_terminals.tabs.get(&tab)?;
        Some((tab, shown.active.clone()))
    }

    /// The folder a new tab in `workspace` starts in: the one the Herdr
    /// workspace started in, as Herdr's own new tabs do, or the folder of the
    /// herdr-core pane the window shows, for a workspace Herdr does not keep.
    fn core_workspace_folder(&self, workspace: &str, cx: &App) -> Option<PathBuf> {
        let herdr = self
            .live
            .snapshot
            .as_deref()
            .and_then(|snapshot| crate::palette::launch_root(snapshot, workspace))
            .map(PathBuf::from)
            .filter(|root| root.is_absolute());
        herdr.or_else(|| {
            let (tab, terminal) = self.shown_core_pane(cx)?;
            let shown = cx.try_global::<crate::browser::Store>()?.get(tab)?;
            (shown.workspace_id == workspace)
                .then(|| self.core_folder(tab, &terminal))
                .flatten()
        })
    }

    /// `workspace` as the window's Herdr daemon keeps it, when it does.
    fn herdr_workspace(&self, workspace: String) -> Option<Herdr> {
        let snapshot = self.live.snapshot.as_deref()?;
        if !snapshot
            .workspaces
            .iter()
            .any(|known| known.workspace_id == workspace)
        {
            return None;
        }
        let client = self
            .endpoints
            .get(self.selected_endpoint)?
            .connection
            .target
            .socket_path()
            .ok()?;
        Some(Herdr {
            api_socket: api_socket(&client)?,
            workspace,
        })
    }

    /// The folder a new tab in `workspace` of the saved host `scope` starts
    /// in, on that host: a kept workspace's own, else the one its Herdr
    /// workspace started in.
    fn remote_workspace_folder(
        &self,
        scope: &crate::browser::Scope,
        workspace: &str,
        cx: &App,
    ) -> Option<PathBuf> {
        if let Some(kept) = cx
            .try_global::<crate::browser::Store>()
            .and_then(|store| store.kept_workspace(scope, workspace))
        {
            return (!kept.cwd.is_empty()).then(|| PathBuf::from(&kept.cwd));
        }
        let index = self.endpoint_of_scope(scope)?;
        let snapshot = self.live_at(index).snapshot.as_deref()?;
        crate::palette::launch_root(snapshot, workspace).map(PathBuf::from)
    }

    /// The endpoint whose workspaces `scope` names.
    fn endpoint_of_scope(&self, scope: &crate::browser::Scope) -> Option<usize> {
        self.endpoints
            .iter()
            .position(|endpoint| crate::browser::scope(endpoint) == *scope)
    }
}
