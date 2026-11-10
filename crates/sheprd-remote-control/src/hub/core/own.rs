//! The layout of Herdr 2 tabs Sheprd keeps none for, as the daemon keeps
//! it: a phone's splits, zooms, and names of terminals no saved tab holds,
//! such as those on a host Sheprd does not run on, or started outside the
//! app. Saved in the daemon's state folder for the core it was made on: a
//! wiped core starts its addresses over, so another core's layout is
//! dropped. Sheprd's saved tabs win: a terminal one holds leaves these.

use super::{arrange::Error, pane_id, projection};
use crate::hub::Layout;
use serde::{Deserialize, Serialize};
use sheprd_backend::TerminalId;
use sheprd_core_client::wire::Terminal as Record;
use sheprd_session::panes::{Axis, Panes};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How a daemon tab's id reads in the layout.
pub(super) const OWN_TAB: &str = "core:own:";
/// The most tabs, and names of each kind, kept.
const MAX_ENTRIES: usize = 256;
/// The largest file read.
const MAX_BYTES: u64 = 1024 * 1024;
/// The longest name kept, in bytes.
const MAX_NAME: usize = 1024;
const FILE: &str = "core-tabs.json";

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Layouts {
    /// The core's id these addresses belong to.
    core: String,
    next: u64,
    tabs: Vec<Tab>,
    /// Pane names by terminal address, a chat's included.
    #[serde(default)]
    names: BTreeMap<String, String>,
    /// Names of the groups the daemon lists (`core:terminals`, a phone's
    /// new workspace), by id.
    #[serde(default)]
    workspaces: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Tab {
    id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    panes: Panes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    zoomed: Option<TerminalId>,
}

/// How long a split's new shell counts as live before a listing shows it:
/// a poll may have listed the core's terminals just before it started.
const FRESH: Duration = Duration::from_secs(10);

/// [`Layouts`] and where they are saved.
#[derive(Debug, Default)]
pub(super) struct Store {
    /// Shells a split started lately, by address, which a listing taken
    /// before they started leaves out.
    fresh: HashMap<String, Instant>,
    pub(super) layouts: Layouts,
    path: Option<PathBuf>,
}

impl Store {
    /// The layouts saved in `state_dir`; none when it holds none readable.
    pub(super) fn load(state_dir: &Path) -> Self {
        let path = state_dir.join(FILE);
        let layouts = read(&path).unwrap_or_default();
        Self {
            fresh: HashMap::new(),
            layouts,
            path: Some(path),
        }
    }

    /// Writes the layouts where they were read. A failure costs only their
    /// survival across a restart.
    pub(super) fn save(&self) -> Result<(), Error> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let bytes =
            serde_json::to_vec(&self.layouts).map_err(sheprd_session::saved::edit::Error::from)?;
        super::arrange::write_atomically(path, &bytes)
    }

    /// Keeps only what `core`'s `records` still hold and none of Sheprd's
    /// saved tabs has taken in (`held`). Answers whether that changed
    /// anything, which is then saved.
    pub(super) fn fit(&mut self, core: &str, records: &[Record], held: &HashSet<String>) -> bool {
        let layouts = &mut self.layouts;
        if layouts.core != core {
            self.fresh.clear();
            let fresh = layouts.core.is_empty();
            *layouts = Layouts {
                core: core.to_owned(),
                ..Layouts::default()
            };
            return !fresh;
        }
        let mut live: HashSet<&str> = records
            .iter()
            .map(|record| record.address.as_str())
            .filter(|address| !held.contains(*address))
            .collect();
        self.fresh
            .retain(|address, at| !live.contains(address.as_str()) && at.elapsed() < FRESH);
        live.extend(
            self.fresh
                .keys()
                .map(String::as_str)
                .filter(|address| !held.contains(*address)),
        );
        let before = (layouts.tabs.len(), layouts.names.len());
        let mut changed = false;
        layouts.tabs.retain_mut(|tab| {
            let dead: Vec<TerminalId> = tab
                .panes
                .terminals()
                .into_iter()
                .filter(|terminal| !live.contains(terminal.as_str()))
                .cloned()
                .collect();
            for terminal in &dead {
                // The last pane stays in the tree, so a tab of none goes.
                if !tab.panes.remove(terminal) {
                    return false;
                }
                changed = true;
                if tab.zoomed.as_ref() == Some(terminal) {
                    tab.zoomed = None;
                }
            }
            tab.panes.count() > 1 || tab.name.is_some() || tab.zoomed.is_some()
        });
        layouts
            .names
            .retain(|address, _| live.contains(address.as_str()));
        changed || before != (layouts.tabs.len(), layouts.names.len())
    }

    /// Puts `new` beside `at`, in `at`'s tab or a new one of the two.
    pub(super) fn split(
        &mut self,
        at: &TerminalId,
        axis: Axis,
        new: TerminalId,
    ) -> Result<(), Error> {
        let address = new.as_str().to_owned();
        let tab = self.tab_mut(at)?;
        tab.panes.split(at, axis, new)?;
        tab.zoomed = None;
        self.fresh.insert(address, Instant::now());
        Ok(())
    }

    /// Whether `at`'s tab could take another pane.
    pub(super) fn room_beside(&self, at: &TerminalId) -> bool {
        self.layouts
            .tabs
            .iter()
            .find(|tab| tab.panes.contains(at))
            .is_none_or(|tab| tab.panes.count() < sheprd_session::panes::MAX_PANES)
    }

    /// A zoomed tab shows its split again, whichever pane asks; otherwise
    /// `at` fills it.
    pub(super) fn zoom(&mut self, at: &TerminalId) -> Result<(), Error> {
        let tab = self.tab_mut(at)?;
        tab.zoomed = match tab.zoomed {
            Some(_) => None,
            None => Some(at.clone()),
        };
        Ok(())
    }

    /// Names the tab of `at`; `None` gives back its program's title.
    pub(super) fn rename_tab(&mut self, at: &TerminalId, name: Option<&str>) -> Result<(), Error> {
        let name = bounded(name);
        self.tab_mut(at)?.name = name;
        Ok(())
    }

    /// Names the pane of `at`, a chat's included.
    pub(super) fn rename_pane(&mut self, at: &TerminalId, name: Option<&str>) -> Result<(), Error> {
        let names = &mut self.layouts.names;
        match bounded(name) {
            Some(name) if names.len() < MAX_ENTRIES || names.contains_key(at.as_str()) => {
                names.insert(at.as_str().to_owned(), name);
            }
            Some(_) => return Err(Error::Full),
            None => {
                names.remove(at.as_str());
            }
        }
        Ok(())
    }

    /// Names one of the daemon's groups.
    pub(super) fn rename_workspace(&mut self, id: &str, name: &str) -> Result<(), Error> {
        let workspaces = &mut self.layouts.workspaces;
        if workspaces.len() >= MAX_ENTRIES && !workspaces.contains_key(id) {
            return Err(Error::Full);
        }
        let name = bounded(Some(name)).ok_or(Error::Name)?;
        workspaces.insert(id.to_owned(), name);
        Ok(())
    }

    /// The tab holding `at`, made when it is in none.
    fn tab_mut(&mut self, at: &TerminalId) -> Result<&mut Tab, Error> {
        let layouts = &mut self.layouts;
        if let Some(index) = layouts.tabs.iter().position(|tab| tab.panes.contains(at)) {
            return Ok(&mut layouts.tabs[index]);
        }
        if layouts.tabs.len() >= MAX_ENTRIES {
            return Err(Error::Full);
        }
        layouts.next += 1;
        layouts.tabs.push(Tab {
            id: layouts.next,
            name: None,
            panes: Panes::single(at.clone()),
            zoomed: None,
        });
        layouts.tabs.last_mut().ok_or(Error::Full)
    }
}

impl Layouts {
    /// Joins each tab's terminals that `layout` lists on their own into one
    /// tab, where its first such terminal was, and shows the names given.
    /// A terminal in one of Sheprd's tabs is listed there and stays.
    pub(super) fn apply(&self, layout: &mut Layout, boot: &str, records: &[Record]) {
        for tab in &self.tabs {
            let id = format!("{OWN_TAB}{}", tab.id);
            let mut members = Vec::new();
            for terminal in tab.panes.terminals() {
                let single = pane_id(boot, terminal.as_str());
                let Some(record) = records
                    .iter()
                    .find(|record| record.address == terminal.as_str())
                else {
                    continue;
                };
                if lone_row(layout, &single).is_some() {
                    members.push((single, record));
                }
            }
            let Some(((lead, _), rest)) = members.split_first() else {
                continue;
            };
            for (single, _) in rest {
                if let Some((workspace, index)) = lone_row(layout, single) {
                    layout.workspaces[workspace].terminals.remove(index);
                }
            }
            let records: Vec<&Record> = members.iter().map(|(_, record)| *record).collect();
            let title = tab.name.as_deref().unwrap_or_default();
            let Some(mut merged) =
                projection::terminal(boot, &id, title, &records, projection::urgency)
            else {
                continue;
            };
            merged.zoomed = tab.zoomed.is_some();
            if let Some((workspace, index)) = lone_row(layout, lead) {
                layout.workspaces[workspace].terminals[index] = merged;
            }
        }
        // A named group with nothing in it yet still carries its name, for
        // the stand-in a phone sees when nothing else is listed.
        if let Some(name) = self.workspaces.get(super::UNLISTED)
            && layout.workspace(super::UNLISTED).is_none()
        {
            layout.workspaces.push(crate::hub::Workspace {
                branch: None,
                chats: Vec::new(),
                cwd: String::new(),
                id: super::UNLISTED.to_owned(),
                label: name.clone(),
                session: None,
                terminals: Vec::new(),
                worktree: None,
            });
        }
        for workspace in &mut layout.workspaces {
            if let Some(name) = self.workspaces.get(&workspace.id) {
                name.clone_into(&mut workspace.label);
            }
            for terminal in workspace
                .terminals
                .iter_mut()
                .filter(|terminal| !terminal.tab.starts_with(super::saved::SAVED_TAB))
            {
                for pane in &mut terminal.panes {
                    let name = records
                        .iter()
                        .find(|record| pane_id(boot, &record.address) == pane.pane)
                        .and_then(|record| self.names.get(&record.address));
                    if let Some(name) = name {
                        pane.label = Some(name.clone());
                    }
                }
            }
        }
    }
}

/// Where `layout` lists the terminal whose lone tab is `single`.
fn lone_row(layout: &Layout, single: &str) -> Option<(usize, usize)> {
    layout
        .workspaces
        .iter()
        .enumerate()
        .find_map(|(workspace, row)| {
            row.terminals
                .iter()
                .position(|terminal| terminal.tab == single)
                .map(|index| (workspace, index))
        })
}

fn bounded(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            let mut end = name.len().min(MAX_NAME);
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            name[..end].to_owned()
        })
}

fn read(path: &Path) -> Option<Layouts> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let layouts: Layouts = serde_json::from_slice(&bytes).ok()?;
    let bounded = layouts.tabs.len() <= MAX_ENTRIES
        && layouts.names.len() <= MAX_ENTRIES
        && layouts.workspaces.len() <= MAX_ENTRIES;
    bounded.then_some(layouts)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
