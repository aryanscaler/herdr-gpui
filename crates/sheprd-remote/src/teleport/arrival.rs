//! A destination receipt is committed before source retirement. It contains
//! only the app-owned destination layout; replay never sends input, relaunches
//! an agent, or retires a source whose state may have changed after a crash.

mod error;
use super::job::{Landed, Outcome};
pub use error::{Error, Result};
use serde::{Deserialize, Serialize};
use sheprd_backend::TerminalId;
use sheprd_session::{SavedTabs, panes::Panes};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_PENDING: usize = 64;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    version: u32,
    scope: String,
    endpoint_id: String,
    workspace: Landed,
    repo_key: String,
    branch: String,
    core_tabs: Vec<Panes>,
}

impl Receipt {
    pub fn new(scope: &str, outcome: &Outcome) -> Self {
        Self {
            version: 1,
            scope: scope.into(),
            endpoint_id: outcome.endpoint_id.clone(),
            workspace: outcome.workspace.clone(),
            repo_key: outcome.repo_key.clone(),
            branch: outcome.branch.clone(),
            core_tabs: outcome.core_tabs.clone(),
        }
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// Recovery adopts existing destination terminals, never resumes again.
    pub fn outcome(&self) -> Outcome {
        Outcome {
            endpoint_id: self.endpoint_id.clone(),
            workspace: self.workspace.clone(),
            repo_key: self.repo_key.clone(),
            branch: self.branch.clone(),
            core_tabs: self.core_tabs.clone(),
            warnings: Vec::new(),
            source_retired: false,
        }
    }

    /// Whether every terminal of the receipt, apart from `closed`, has a
    /// tab of its host saved in `bytes`.
    pub fn saved_in(&self, bytes: &[u8], closed: &[TerminalId]) -> bool {
        SavedTabs::parse(bytes).is_ok_and(|saved| {
            self.core_tabs
                .iter()
                .flat_map(Panes::terminals)
                .filter(|terminal| !closed.contains(terminal))
                .all(|terminal| {
                    saved
                        .core_tabs(&self.scope)
                        .any(|tab| tab.panes.contains(terminal))
                })
        })
    }
}

/// Called on the move worker before retirement, not on the UI thread. File and
/// rename are synced: a failed receipt leaves the source running.
pub fn save(directory: &Path, receipt: &Receipt) -> Result<PathBuf> {
    fs::create_dir_all(directory)?;
    if fs::read_dir(directory)?.take(MAX_PENDING).count() >= MAX_PENDING {
        return Err(Error::Full);
    }
    let bytes = serde_json::to_vec(receipt)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    let path = directory.join(format!("{}.json", uuid::Uuid::new_v4()));
    file.persist_noclobber(&path).map_err(|error| error.error)?;
    #[cfg(unix)]
    fs::File::open(directory)?.sync_all()?;
    Ok(path)
}

pub fn load(directory: &Path) -> Result<Vec<(PathBuf, Receipt)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut receipts = Vec::new();
    for entry in entries.take(MAX_PENDING) {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        match read(&path) {
            Ok(receipt) => receipts.push((path, receipt)),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "Could not recover a Teleport arrival")
            }
        }
    }
    Ok(receipts)
}

fn read(path: &Path) -> Result<Receipt> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    let receipt: Receipt = serde_json::from_slice(&bytes)?;
    if receipt.version != 1 {
        return Err(Error::Version);
    }
    Ok(receipt)
}

/// A receipt is released only once all destination terminals are saved. A
/// failed browser-tab write keeps it recoverable at the next app start.
/// Removes the receipt once its tabs are saved. `closed` names terminals
/// whose tabs the user closed after this run adopted the receipt: they are not
/// waited for, so a closed tab is not reopened on the next launch.
pub fn release(
    path: &Path,
    receipt: &Receipt,
    saved_path: &Path,
    closed: &[TerminalId],
) -> Result<bool> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(saved_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut saved = Vec::new();
    (&file)
        .take(sheprd_session::MAX_SAVED_BYTES as u64 + 1)
        .read_to_end(&mut saved)?;
    if !receipt.saved_in(&saved, closed) {
        return Ok(false);
    }
    file.sync_all()?;
    #[cfg(unix)]
    if let Some(parent) = saved_path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
