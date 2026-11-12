//! Serialize mutations across windows/processes. The OS releases the lease
//! after a crash, unlike a persistent busy bit in the run database.
use std::{
    fs::File,
    path::PathBuf,
    time::{Duration, Instant},
};

/// How long a taken lease may outlive its holder. A process spawned on any
/// thread inherits the lock's file until it execs, so a lease just dropped
/// can still be held for that moment; only a lease held past this is busy.
const RELEASE_GRACE: Duration = Duration::from_secs(1);
const RETRY: Duration = Duration::from_millis(10);

#[derive(Debug, thiserror::Error)]
#[error("Cannot lock orchestrator actions at {path}: {source}")]
pub struct Error {
    path: PathBuf,
    #[source]
    source: std::io::Error,
}

pub(super) fn take(site: &super::Site) -> crate::Result<File> {
    let path = site.database.with_extension("actions.lock");
    crate::private_file::prepare(&path)?;
    let file = File::options()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|source| Error {
            path: path.clone(),
            source,
        })?;
    let deadline = Instant::now() + RELEASE_GRACE;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(RETRY);
            }
            Err(std::fs::TryLockError::WouldBlock) => return Err(crate::Error::Busy),
            Err(std::fs::TryLockError::Error(source)) => {
                return Err(Error { path, source }.into());
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
