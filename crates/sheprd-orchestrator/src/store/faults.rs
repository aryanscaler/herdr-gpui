//! Deterministic session-write failures, scoped to one test's dispatch thread.
//! Only the Unix-only Herdr 2 dispatch tests arm them.
use std::cell::Cell;
#[cfg(unix)]
use std::{marker::PhantomData, rc::Rc};

thread_local! {
    static PASSING: Cell<usize> = const { Cell::new(0) };
    static REMAINING: Cell<usize> = const { Cell::new(0) };
}

#[cfg(unix)]
pub(crate) struct Failure(PhantomData<Rc<()>>);

#[cfg(unix)]
impl Drop for Failure {
    fn drop(&mut self) {
        PASSING.set(0);
        REMAINING.set(0);
    }
}

/// Fails `count` session writes after letting `passing` through, so a test
/// can fail the write that records a started agent rather than the receipt
/// written before anything was created.
#[cfg(unix)]
pub(crate) fn session_updates_after(passing: usize, count: usize) -> Failure {
    assert_eq!(REMAINING.replace(count), 0);
    PASSING.set(passing);
    Failure(PhantomData)
}

#[cfg(unix)]
pub(crate) fn remaining() -> usize {
    REMAINING.get()
}

pub(super) fn before_session_update() -> crate::Result<()> {
    let left = REMAINING.get();
    if left == 0 {
        return Ok(());
    }
    if let Some(passing) = PASSING.get().checked_sub(1) {
        PASSING.set(passing);
        return Ok(());
    }
    REMAINING.set(left - 1);
    Err(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
        Some(format!("injected session write failure {left}")),
    )
    .into())
}
