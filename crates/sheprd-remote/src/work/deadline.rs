//! A wall-clock budget shared by every query of one checkout identity.
//! Host scripts expose cancellation, so a scoped watcher also stops a chatty
//! or blocked child at the deadline rather than merely shortening idle time.

use super::{Error, Host, Result, Step, identity, script};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// Read identity before `deadline`, observing external cancellation too.
/// Blocking; use only on a background worker.
pub fn identity_before(
    host: &Host,
    checkout: &str,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<sheprd_vcs::git::Identity> {
    if cancelled.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(timeout());
    }
    let stop = AtomicBool::new(false);
    let expired = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let (done, waiting) = mpsc::sync_channel(1);
        let stop = &stop;
        let expired = &expired;
        std::thread::Builder::new()
            .name("worktree-deadline".into())
            .spawn_scoped(scope, move || {
                loop {
                    if cancelled.load(Ordering::Acquire) {
                        stop.store(true, Ordering::Release);
                        return;
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        expired.store(true, Ordering::Release);
                        stop.store(true, Ordering::Release);
                        return;
                    }
                    match waiting.recv_timeout(remaining.min(Duration::from_millis(10))) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            })
            .map_err(|source| script(Step::List)(sheprd_client::Error::ScriptSpawn(source)))?;
        let result = identity(host, checkout, stop);
        let _ = done.send(());
        if expired.load(Ordering::Acquire) || Instant::now() >= deadline {
            Err(timeout())
        } else {
            result
        }
    })
}

fn timeout() -> Error {
    script(Step::List)(sheprd_client::Error::ScriptTimeout)
}
