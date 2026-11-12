use super::*;
use std::time::Duration;

/// How long a test waits for a lock just released. A process another test
/// spawns holds the lock's file until it execs, so a released lock can
/// linger for that moment; only checks that it is still held use no wait.
const RELEASED: Duration = Duration::from_secs(1);

#[test]
fn an_expired_startup_budget_never_reads_or_writes_a_phones_inflight_layout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("browser-tabs.json");
    let phone = own(&path, Duration::ZERO).unwrap();
    // Simulate the startup deadline expiring while the phone still holds the
    // lock across terminal.run. No sleep or real terminal is needed.
    let result = Store::load_at(Some(path.clone()), Duration::ZERO);
    assert!(matches!(result, Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock));
    assert!(!path.exists(), "startup must not create an unlocked writer");

    let saved = serde_json::json!({"tabs": [{
        "id": 4, "scope": "local:/test.sock", "workspace_id": "w1", "title": "shell",
        "location": {"kind": "core_terminal", "panes": {"axis": "row", "first": "t1", "second": "t2"},
            "active": "t2", "zoomed": "t2", "name": "phone", "names": {"t2": "logs"}}
    }], "workspaces": []});
    std::fs::write(&path, saved.to_string()).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        Store::load_at(Some(path.clone()), Duration::ZERO),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(phone);
    let mut store = Store::load_at(Some(path.clone()), RELEASED).unwrap();
    let tab = store.get(TabId::test(4)).unwrap();
    let restored = serde_json::to_value(&tab.location).unwrap();
    assert_eq!(restored["panes"]["second"], "t2");
    assert_eq!(restored["active"], "t2");
    assert_eq!(restored["zoomed"], "t2");
    assert_eq!(restored["name"], "phone");
    assert_eq!(restored["names"]["t2"], "logs");
    // Ownership stays held until the writer has stopped, including on reopen.
    assert!(
        matches!(own(&path, Duration::ZERO), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    store.writer.take().unwrap().finish();
    drop(store);
    assert!(own(&path, RELEASED).is_ok());
}

#[test]
fn first_launch_creates_the_lock_parent_before_starting_its_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new/browser-tabs.json");
    let mut store = Store::load_at(Some(path.clone()), Duration::ZERO).unwrap();
    assert!(sheprd_session::saved::lock_path(&path).exists());
    assert!(store.tabs.is_empty());
    store.writer.take().unwrap().finish();
}
