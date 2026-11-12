use super::*;

fn site(dir: &std::path::Path) -> crate::actions::Site {
    crate::actions::Site {
        host: sheprd_remote::teleport::Host::new(&sheprd_client::ConnectTarget::Local).unwrap(),
        target: sheprd_client::ConnectTarget::Local,
        main_root: String::new(),
        database: dir.join("private/state.sqlite3"),
    }
}

#[test]
fn another_window_cannot_mutate_until_the_os_lease_is_released() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path());
    let lease = take(&site).unwrap();
    assert!(matches!(take(&site), Err(crate::Error::Busy)));
    drop(lease);
    assert!(take(&site).is_ok());
}

/// A spawned child keeps the lock's file until it execs, so a lease can
/// linger an instant after its holder dropped it; that is not another window.
#[test]
fn a_lease_released_moments_later_is_not_busy() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path());
    let lingering = take(&site).unwrap();
    let (taking, started) = std::sync::mpsc::channel();
    let release = std::thread::spawn(move || {
        started.recv().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        drop(lingering);
    });
    taking.send(()).unwrap();
    assert!(take(&site).is_ok());
    release.join().unwrap();
}
