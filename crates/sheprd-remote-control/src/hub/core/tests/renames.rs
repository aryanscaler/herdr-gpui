//! What Sheprd's renames look like to the daemon: which workspaces it keeps,
//! and the names it gave tabs and panes.

use super::*;

#[test]
fn only_a_workspace_sheprd_keeps_here_is_sheprds_to_rename() {
    let dir = tempfile::tempdir().unwrap();
    let tabs = dir.path().join("browser-tabs.json");
    std::fs::write(
        &tabs,
        json!({"tabs": [], "workspaces": [
            {"id": "space-1", "scope": "local:/test.sock", "label": "api", "cwd": "/code/api"},
            {"id": "space-9", "scope": "local:/other.sock", "label": "x", "cwd": "/x"}
        ]})
        .to_string(),
    )
    .unwrap();
    let source = Source {
        connected: AtomicBool::new(false),
        endpoint: Default::default(),
        holds: AtomicU64::new(0),
        own: Mutex::default(),
        poller: Mutex::default(),
        scope: Some("local:/test.sock".into()),
        state: Mutex::default(),
        tabs: Some(tabs.clone()),
    };
    lock(&source.poller).project("fake-core", &[], Some(&tabs), source.scope.as_deref());
    assert!(source.keeps_workspace("space-1"));
    assert!(!source.keeps_workspace("space-9"), "another host's");
    assert!(!source.keeps_workspace("w1"), "Herdr 1's");
}

#[test]
fn a_pane_the_user_named_shows_its_name_and_a_tab_its_own() {
    let saved = serde_json::from_value(json!({"tabs": [
        {"id": 4, "scope": "local:/test.sock", "workspace_id": "w1", "title": "build",
         "location": {"kind": "core_terminal", "panes": {"axis": "row", "first": "t1", "second": "t2"},
                      "name": "build", "names": {"t2": "server"}}}
    ]}))
    .unwrap();
    let records = ["t1", "t2"].map(|address| serde_json::from_value(record(address)).unwrap());
    let layout = projection::project(
        "fake-core",
        &records,
        Some(&saved),
        Some("local:/test.sock"),
    );
    let tab = &layout.workspace("w1").unwrap().terminals[0];
    assert_eq!(tab.label, "build");
    let labels: Vec<Option<&str>> = tab.panes.iter().map(|pane| pane.label.as_deref()).collect();
    assert_eq!(labels[1], Some("server"));
    assert_ne!(labels[0], Some("server"));
}
