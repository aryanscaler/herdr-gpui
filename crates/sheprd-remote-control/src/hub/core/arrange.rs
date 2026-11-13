//! A phone's split, zoom, rename, or close of a Herdr 2 tab, made by the
//! daemon itself when Sheprd cannot make it: herdr-core keeps no layout.
//!
//! - A tab Sheprd saves is Sheprd's. While it runs, it makes the change
//!   (`control::layout`). While it is closed, the daemon edits the saved
//!   tabs file through `sheprd_session::saved::edit`, touching only that
//!   tab, and Sheprd reads it when it starts. A split's shell is labelled
//!   for the tab's workspace, so a Sheprd that starts meanwhile still lists
//!   it there.
//! - A tab no saved tab holds (on a host, or started outside Sheprd) is
//!   the daemon's own ([`super::own`]), kept until Sheprd takes it in.
//! - A Herdr desktop space's tab is that app's: it is refused.

mod error;

pub use error::Error;

use super::{HubError, Result, Source, lock, own::OWN_TAB, projection, saved::SAVED_TAB};
use crate::{control::layout::Op, hub::Hub};
use sheprd_backend::TerminalId;
use sheprd_core_client::wire::{Run, Terminal};
use sheprd_session::{
    NEW_WORKSPACE_LABEL, SESSION_LABEL, WORKSPACE_LABEL,
    panes::Axis,
    saved::edit::{self, Change},
};
use std::{collections::BTreeMap, io::Read, path::Path, time::SystemTime};

// Serialize phones before taking the cross-process ownership lock.
static EDITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// How often a held saved-tabs lock is tried again before Sheprd is taken
/// to hold it, and how long apart: 100ms in all, on the blocking pool.
const OWNER_RETRIES: u32 = 5;
const OWNER_RETRY: std::time::Duration = std::time::Duration::from_millis(20);

impl From<Error> for HubError {
    fn from(error: Error) -> Self {
        Self::Core(super::Error::Arrange(error))
    }
}

impl HubError {
    pub(crate) fn desktop_owns_layout(&self) -> bool {
        matches!(self, Self::Core(super::Error::Arrange(Error::SheprdOpen)))
    }
}

/// Who changes a Herdr 2 tab's layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Keeper {
    /// Sheprd's saved tab `tab`, at the pane of the terminal at `terminal`.
    Saved { tab: u64, terminal: String },
    /// The daemon, at the pane of the terminal at this address.
    Own(String),
}

/// Whether a phone may split, zoom, and rename the tab `tab`: every tab
/// but a Herdr desktop space's, whose layout that app keeps.
pub(crate) fn arrangeable(tab: &str) -> bool {
    !tab.starts_with("core:app-tab:")
}

fn axis(op: &Op) -> Option<Axis> {
    match op {
        Op::SplitRight => Some(Axis::Row),
        Op::SplitDown => Some(Axis::Column),
        _ => None,
    }
}

impl Source {
    /// The terminal at `address`, as the last poll saw it.
    fn record(&self, address: &str) -> Result<Terminal> {
        lock(&self.state)
            .terminals
            .iter()
            .find(|terminal| terminal.address == address)
            .cloned()
            .ok_or(HubError::Response)
    }

    /// A shell beside `record`: in its folder, labelled for its workspace.
    fn start_beside(&self, record: &Terminal, labels: BTreeMap<&str, &str>) -> Result<Terminal> {
        let cwd = projection::cwd(record);
        self.start_run(&Run {
            cwd: cwd.as_deref(),
            keep: true,
            labels,
            ..Run::default()
        })
    }

    /// Makes `op` at the terminal at `address` in the daemon's own layout.
    /// Answers a split's new terminal.
    pub(super) fn arrange_own(&self, address: &str, op: &Op) -> Result<Option<String>> {
        let at = TerminalId::new(address).map_err(Error::from)?;
        if let Some(axis) = axis(op) {
            if !lock(&self.own).room_beside(&at) {
                return Err(Error::from(sheprd_session::panes::Error::Full).into());
            }
            let record = self.record(address)?;
            // Where its workspace is, for Sheprd to list it there too.
            let labels = [WORKSPACE_LABEL, SESSION_LABEL, NEW_WORKSPACE_LABEL]
                .into_iter()
                .filter_map(|key| Some((key, record.labels.get(key)?.as_str())))
                .collect();
            let new = self.start_beside(&record, labels)?;
            let split = TerminalId::new(new.address.as_str())
                .map_err(Error::from)
                .and_then(|id| lock(&self.own).split(&at, axis, id));
            if let Err(error) = split {
                self.end(&new.address);
                return Err(error.into());
            }
            self.save_own();
            return Ok(Some(new.address));
        }
        {
            let mut own = lock(&self.own);
            match op {
                Op::Zoom => own.zoom(&at)?,
                Op::RenameTab(name) => own.rename_tab(&at, name.as_deref())?,
                Op::RenamePane(name) => own.rename_pane(&at, name.as_deref())?,
                // Ending its terminals is all closing it takes.
                Op::Close => return Ok(None),
                Op::SplitDown | Op::SplitRight | Op::RenameWorkspace { .. } => {
                    return Err(super::Error::Unsupported.into());
                }
            }
        }
        self.save_own();
        Ok(None)
    }

    /// Names one of the groups the daemon lists.
    pub(super) fn name_group(&self, workspace: &str, name: &str) -> Result<()> {
        lock(&self.own).rename_workspace(workspace, name)?;
        self.save_own();
        Ok(())
    }

    /// Makes `op` at the terminal at `address` in Sheprd's saved tab `tab`,
    /// writing the saved tabs while Sheprd is closed. Answers a split's new
    /// terminal.
    pub(super) fn arrange_saved(&self, tab: u64, address: &str, op: &Op) -> Result<Option<String>> {
        let (path, scope) = self
            .tabs
            .as_deref()
            .zip(self.scope.as_deref())
            .ok_or(Error::NoTabs)?;
        let _editing = lock(&EDITING);
        // Take ownership before starting a split's shell. A running desktop
        // must receive the intent without a speculative terminal being made.
        let _owner = file_owner(path)?;
        if let Op::RenameWorkspace { workspace, name } = op {
            edit_owned_file(path, |bytes| {
                edit::rename_workspace(bytes, scope, workspace, name.trim())
            })?;
            return Ok(None);
        }
        let at = TerminalId::new(address).map_err(Error::from)?;
        // The phone names a recovered chat's replacement, which a file saved
        // before the recovery does not: rebind it within the same edit.
        let replacements = lock(&self.poller).replacements.clone();
        let remapped = |bytes: &[u8]| -> std::result::Result<Vec<u8>, edit::Error> {
            if replacements.is_empty() {
                return Ok(bytes.to_vec());
            }
            edit::remap(bytes, scope, &replacements)
        };
        if let Some(axis) = axis(op) {
            let record = self.record(address)?;
            let workspace = lock(&self.poller).saved_workspace(scope, tab);
            let labels = workspace
                .as_deref()
                .filter(|id| sheprd_session::tab_workspace(id))
                .map(|id| BTreeMap::from([(WORKSPACE_LABEL, id)]))
                .unwrap_or_default();
            let new = self.start_beside(&record, labels)?;
            let split = TerminalId::new(new.address.as_str())
                .map_err(Error::from)
                .and_then(|id| {
                    edit_owned_file(path, |bytes| {
                        let change = Change::Split {
                            at: &at,
                            axis,
                            new: id,
                        };
                        edit::edit_tab(&remapped(bytes)?, scope, tab, change)
                    })
                });
            if let Err(error) = split {
                self.end(&new.address);
                return Err(error.into());
            }
            return Ok(Some(new.address));
        }
        let change = match op {
            Op::Zoom => Change::Zoom(&at),
            Op::RenameTab(name) => Change::Rename(name.as_deref()),
            Op::RenamePane(name) => Change::RenamePane {
                at: &at,
                name: name.as_deref(),
            },
            Op::Close => Change::Close,
            Op::SplitDown | Op::SplitRight | Op::RenameWorkspace { .. } => {
                return Err(super::Error::Unsupported.into());
            }
        };
        edit_owned_file(path, |bytes| {
            edit::edit_tab(&remapped(bytes)?, scope, tab, change)
        })?;
        Ok(None)
    }

    fn save_own(&self) {
        if let Err(error) = lock(&self.own).save() {
            tracing::warn!(%error, "The phone's Herdr 2 tab layouts were not saved");
        }
    }

    /// Ends a shell a failed split started, so none is left unlisted.
    fn end(&self, address: &str) {
        let core = lock(&self.state).core.clone();
        if let Some(Err(error)) = core.map(|core| core.kill(address)) {
            tracing::warn!(%error, "A split's shell stayed after the split failed");
        }
    }
}

impl Hub {
    /// The file lock is released on desktop exit, before its last heartbeat
    /// expires. Probe on the blocking pool and serialize with our own edits.
    pub(crate) async fn desktop_layout_owner(&self) -> Result<Option<bool>> {
        let Some(core) = self.core.clone() else {
            return Ok(None);
        };
        super::super::on_core(move || {
            let Some(path) = core.tabs.as_deref() else {
                return Ok(None);
            };
            let _editing = lock(&EDITING);
            match file_owner(path) {
                Ok(_) => Ok(Some(false)),
                Err(Error::SheprdOpen) => Ok(Some(true)),
                Err(error) => Err(error.into()),
            }
        })
        .await
    }

    /// Who changes the layout of the Herdr 2 tab `tab` holding `pane`;
    /// `None` when the pane is not Herdr 2's.
    pub(crate) fn core_keeper(&self, tab: &str, pane: &str) -> Result<Option<Keeper>> {
        if !super::owns(&serde_json::json!({ "pane_id": pane })) {
            return Ok(None);
        }
        if !arrangeable(tab) {
            return Err(Error::Fixed.into());
        }
        let core = self.core.as_ref().ok_or(super::Error::Disconnected)?;
        if tab.starts_with(SAVED_TAB) {
            let (tab, terminal) = core
                .saved_target(tab, pane)
                .ok_or(super::Error::Unsupported)?;
            return Ok(Some(Keeper::Saved { tab, terminal }));
        }
        debug_assert!(tab.starts_with(OWN_TAB) || tab == pane || tab.starts_with("core:"));
        let (_, terminal) = core.terminal(pane)?;
        Ok(Some(Keeper::Own(terminal.address)))
    }

    /// Makes `op` where `keeper` says, Sheprd aside. Answers a split's new
    /// terminal, by its address.
    pub(crate) async fn arrange(&self, keeper: Keeper, op: Op) -> Result<Option<String>> {
        let core = self.core.clone().ok_or(super::Error::Disconnected)?;
        super::super::on_core(move || match keeper {
            Keeper::Saved { tab, terminal } => core.arrange_saved(tab, &terminal, &op),
            Keeper::Own(terminal) => core.arrange_own(&terminal, &op),
        })
        .await
    }

    /// Names one of the groups the daemon lists for Herdr 2: its unlisted
    /// terminals, or a phone's new workspace. `false` for any other.
    pub(crate) async fn rename_core_group(&self, workspace: &str, name: &str) -> Result<bool> {
        if workspace != super::UNLISTED && !super::is_new_workspace(workspace) {
            return Ok(false);
        }
        let core = self.core.clone().ok_or(super::Error::Disconnected)?;
        let (workspace, name) = (workspace.to_owned(), name.to_owned());
        super::super::on_core(move || core.name_group(&workspace, &name)).await?;
        Ok(true)
    }
}

/// Changes the file at `path` by `change`, unless it changed meanwhile.
#[cfg(test)]
fn edit_file(
    path: &Path,
    change: impl FnOnce(&[u8]) -> std::result::Result<Vec<u8>, edit::Error>,
) -> std::result::Result<(), Error> {
    // Phones' edits run on the blocking pool side by side: one at a time,
    // or two could read the same file and the later write drop the other.
    let _editing = lock(&EDITING);
    let _owner = file_owner(path)?;
    edit_owned_file(path, change)
}

fn file_owner(path: &Path) -> std::result::Result<std::fs::File, Error> {
    // Sheprd holds this from before it reads its tabs until it quits: one
    // starting or running now would overwrite the edit, so it is refused.
    let owner = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(sheprd_session::saved::lock_path(path))?;
    // The lock is the open file's, which a child another thread forks
    // shares until it runs its program, as the daemon's `git` does: a lock
    // just let go can read as held for that moment. Sheprd holds its own
    // for as long as it runs, so a few short retries tell the two apart.
    let mut retries = 0;
    loop {
        match owner.try_lock() {
            Ok(()) => return Ok(owner),
            Err(std::fs::TryLockError::WouldBlock) if retries < OWNER_RETRIES => {
                retries += 1;
                std::thread::sleep(OWNER_RETRY);
            }
            Err(std::fs::TryLockError::WouldBlock) => return Err(Error::SheprdOpen),
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

fn edit_owned_file(
    path: &Path,
    change: impl FnOnce(&[u8]) -> std::result::Result<Vec<u8>, edit::Error>,
) -> std::result::Result<(), Error> {
    // The modified time catches anything else writing meanwhile.
    let modified =
        |path: &Path| -> std::io::Result<SystemTime> { std::fs::metadata(path)?.modified() };
    let before = modified(path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(sheprd_session::MAX_SAVED_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let changed = change(&bytes)?;
    if modified(path)? != before {
        return Err(Error::Changed);
    }
    write_atomically(path, &changed)
}

/// Writes `bytes` to `path` whole or not at all: a reader never sees half.
pub(super) fn write_atomically(path: &Path, bytes: &[u8]) -> std::result::Result<(), Error> {
    use std::io::Write as _;
    let dir = path.parent().ok_or(Error::NoTabs)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| Error::Io(error.error))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
