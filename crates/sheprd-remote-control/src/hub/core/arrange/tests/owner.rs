//! Whether Sheprd holds its saved tabs, told apart from a lock that only
//! looks held for a moment.

use super::*;

/// The saved tabs' lock file, locked as Sheprd or a forked child holds it.
fn held(path: &Path) -> std::fs::File {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(sheprd_session::saved::lock_path(path))
        .unwrap();
    lock.try_lock().unwrap();
    lock
}

#[test]
fn a_lock_let_go_a_moment_later_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let lock = held(&path);
    // As a child forked with the lock's file open lets go once it runs
    // its program.
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        drop(lock);
    });
    assert!(file_owner(&path).is_ok());
    release.join().unwrap();
}

#[test]
fn a_lock_held_throughout_is_sheprds() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let _sheprd = held(&path);
    assert!(matches!(file_owner(&path), Err(Error::SheprdOpen)));
}
