//! The app's saved tabs, as anything other than the app reads them. Only
//! tabs of herdr-core terminals matter here, so every other kind of tab, and
//! one that fails to read, is skipped rather than costing the rest.

pub mod edit;
mod error;

pub use error::Error;

use crate::panes::Panes;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The lock beside the saved tabs at `tabs`. The app holds it from before
/// it reads them until it quits, as it alone writes them then; anything
/// else edits them only while it can take the lock itself.
pub fn lock_path(tabs: &std::path::Path) -> std::path::PathBuf {
    tabs.with_extension("lock")
}

/// The largest saved tabs file read.
pub const MAX_SAVED_BYTES: usize = 1024 * 1024;
/// The most saved tabs read.
pub const MAX_SAVED_TABS: usize = 256;

#[derive(Debug, Deserialize)]
pub struct SavedTabs {
    tabs: Vec<SavedTab>,
    /// Files from before the app kept workspaces have none.
    #[serde(default, deserialize_with = "readable_workspaces")]
    workspaces: Vec<SavedWorkspace>,
}

/// A workspace the app keeps for herdr-core terminals, which keep none. The
/// app writes them beside its tabs in the order the user arranged them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedWorkspace {
    /// Where its new terminals start.
    pub cwd: String,
    /// Whether the user named it; otherwise the app named it after its
    /// folder, and a linked worktree's row shows its branch instead. Always
    /// written, so a record without it is from before it was, and keeps the
    /// name it has.
    #[serde(default = "named")]
    pub custom: bool,
    /// Unique among the app's workspaces; its tabs name it.
    pub id: String,
    pub label: String,
    /// The host it belongs to, as the app names hosts in its saved tabs.
    pub scope: String,
}

impl SavedWorkspace {
    /// Whether a saved workspace is one the app could have written.
    pub fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= MAX_ID_BYTES
            && !self.scope.is_empty()
            && self.label.chars().count() <= MAX_LABEL_CHARS
            && self.cwd.len() <= MAX_CWD_BYTES
    }
}

fn named() -> bool {
    true
}

/// The saved workspaces that read, each on its own, so one damaged record
/// costs only itself rather than every tab and workspace in the file. For
/// any reader of the file, the app's own included.
pub fn readable_workspaces<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SavedWorkspace>, D::Error> {
    let values = Vec::<Value>::deserialize(deserializer)?;
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_value::<SavedWorkspace>(value).ok())
        .filter(SavedWorkspace::valid)
        .collect())
}

/// The longest workspace ID kept, as for a tab's workspace.
pub const MAX_ID_BYTES: usize = 256;
/// The herdr-core label naming the workspace a terminal was started for
/// outside the app, as by a phone: the app puts such a terminal in a tab
/// of that workspace, and the remote control daemon lists it there.
pub const WORKSPACE_LABEL: &str = "sheprd.workspace";

/// With [`WORKSPACE_LABEL`], the named Herdr 1 session on this computer
/// whose workspace it is; absent for the primary session or the app's own.
pub const SESSION_LABEL: &str = "sheprd.session";

/// The herdr-core label asking the app to keep a new workspace in this
/// folder for a terminal started outside it, as a phone's new workspace
/// is; the app puts the terminal in a tab there.
pub const NEW_WORKSPACE_LABEL: &str = "sheprd.new_workspace";

/// The folder a terminal's labels ask a new workspace for, when it is one
/// the app could keep: absolute, and within its bounds.
pub fn new_workspace(labels: &BTreeMap<String, String>) -> Option<&str> {
    labels
        .get(NEW_WORKSPACE_LABEL)
        .map(String::as_str)
        .filter(|cwd| cwd.len() <= MAX_CWD_BYTES && std::path::Path::new(cwd).is_absolute())
}

/// A workspace a terminal was started for, as its labels name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Labelled<'a> {
    /// The workspace's id within its session.
    pub id: &'a str,
    /// The named Herdr 1 session, or `None` for the primary or the app.
    pub session: Option<&'a str>,
}

/// The workspace a terminal's `labels` name, when it is one the app could
/// keep a tab in: a Herdr 1 workspace or one the app keeps.
pub fn labelled_workspace(labels: &BTreeMap<String, String>) -> Option<Labelled<'_>> {
    let id = labels
        .get(WORKSPACE_LABEL)
        .map(String::as_str)
        .filter(|id| tab_workspace(id))?;
    let session = match labels.get(SESSION_LABEL) {
        Some(name) if name.is_empty() || name.len() > MAX_ID_BYTES || name.contains(':') => {
            return None;
        }
        session => session.map(String::as_str),
    };
    Some(Labelled { id, session })
}

/// Whether `id` names a workspace the app could keep a tab in, rather than
/// a group the remote control daemon makes up (`core:…`).
pub fn tab_workspace(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID_BYTES && !id.starts_with("core:")
}
/// The longest workspace name kept.
pub const MAX_LABEL_CHARS: usize = 200;
/// The longest folder kept for a workspace.
pub const MAX_CWD_BYTES: usize = 4096;

#[derive(Debug, Deserialize)]
struct SavedTab {
    id: u64,
    #[serde(default, deserialize_with = "core_location")]
    location: Option<CoreLocation>,
    scope: String,
    title: String,
    workspace_id: String,
}

/// The `core_terminal` kind of the app's `Location`. An enum because only
/// an enum's tag is checked: any other kind fails, and reads as none.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CoreLocation {
    CoreTerminal {
        /// Names the user gave panes, by terminal, shown over their titles.
        #[serde(default, deserialize_with = "pane_names")]
        names: BTreeMap<String, String>,
        /// The tab's name, shown over its program's title.
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        observe: bool,
        #[serde(alias = "terminal")]
        panes: Panes,
        /// The pane filling the tab.
        #[serde(default)]
        zoomed: Option<sheprd_backend::TerminalId>,
    },
}

/// At most one name per pane a tab can hold, each bounded; a tab with more
/// keeps none rather than fail to read.
fn pane_names<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    let names = BTreeMap::<String, String>::deserialize(deserializer)?;
    let bounded = names.len() <= MAX_SAVED_TABS
        && names.iter().all(|(terminal, name)| {
            terminal.len() <= MAX_ID_BYTES && name.len() <= MAX_ID_BYTES * 4
        });
    Ok(if bounded { names } else { BTreeMap::new() })
}

fn core_location<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<CoreLocation>, D::Error> {
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}

/// One tab of herdr-core terminals that types into them.
#[derive(Clone, Copy, Debug)]
pub struct CoreTab<'a> {
    pub id: u64,
    pub panes: &'a Panes,
    pub title: &'a str,
    pub workspace_id: &'a str,
}

impl SavedTabs {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_SAVED_BYTES {
            return Err(Error::Large);
        }
        let saved: Self = serde_json::from_slice(bytes)?;
        if saved.tabs.len() > MAX_SAVED_TABS || saved.workspaces.len() > MAX_SAVED_TABS {
            return Err(Error::Many);
        }
        Ok(saved)
    }

    /// `scope`'s workspaces the app keeps, in the user's order. One that
    /// fails to read is left out.
    pub fn workspaces<'a>(&'a self, scope: &'a str) -> impl Iterator<Item = &'a SavedWorkspace> {
        self.workspaces
            .iter()
            .filter(move |workspace| workspace.scope == scope && workspace.valid())
    }

    /// Names the user gave the panes of `scope`'s tab `id`, by terminal
    /// address, shown over their programs' titles.
    pub fn pane_names(&self, scope: &str, id: u64) -> Option<&BTreeMap<String, String>> {
        match self.location(scope, id)? {
            CoreLocation::CoreTerminal { names, .. } => Some(names),
        }
    }

    /// `scope`'s tabs in a workspace, in saved order. Mirrors are left out:
    /// the terminal they watch is listed where it types.
    pub fn core_tabs<'a>(&'a self, scope: &'a str) -> impl Iterator<Item = CoreTab<'a>> {
        self.tabs
            .iter()
            .filter(move |tab| tab.scope == scope && !tab.workspace_id.is_empty())
            .filter_map(|tab| {
                let CoreLocation::CoreTerminal { observe, panes, .. } = tab.location.as_ref()?;
                (!observe).then_some(CoreTab {
                    id: tab.id,
                    panes,
                    title: &tab.title,
                    workspace_id: &tab.workspace_id,
                })
            })
    }

    /// The name the user gave `scope`'s tab `id`, shown over its title.
    pub fn tab_name(&self, scope: &str, id: u64) -> Option<&str> {
        match self.location(scope, id)? {
            CoreLocation::CoreTerminal { name, .. } => {
                name.as_deref().filter(|name| !name.trim().is_empty())
            }
        }
    }

    /// The pane filling `scope`'s tab `id`, when one does.
    pub fn tab_zoomed(&self, scope: &str, id: u64) -> Option<&sheprd_backend::TerminalId> {
        match self.location(scope, id)? {
            CoreLocation::CoreTerminal { panes, zoomed, .. } => {
                zoomed.as_ref().filter(|zoomed| panes.contains(zoomed))
            }
        }
    }

    fn location(&self, scope: &str, id: u64) -> Option<&CoreLocation> {
        self.tabs
            .iter()
            .find(|tab| tab.scope == scope && tab.id == id)?
            .location
            .as_ref()
    }
}
