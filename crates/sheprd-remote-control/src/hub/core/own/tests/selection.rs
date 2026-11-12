use super::*;

#[test]
fn split_and_zoom_selection_survive_restart_and_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let mut own = Store::load(dir.path());
    let records = records(&["t1", "t2", "t3"]);
    own.fit("fake-core", &records, &HashSet::new());
    own.split(&id("t1"), Axis::Column, id("t2")).unwrap();
    own.split(&id("t2"), Axis::Row, id("t3")).unwrap();
    own.rename_tab(&id("t1"), Some("remote logs")).unwrap();
    own.rename_pane(&id("t2"), Some("server")).unwrap();
    own.zoom(&id("t2")).unwrap();
    own.save().unwrap();
    let mut own = Store::load(dir.path());
    let shown = layout(&records, None, &own);
    let tab = &shown.workspaces[0].terminals[0];
    assert_eq!(panes(tab), [pane("t1"), pane("t2"), pane("t3")]);
    assert_eq!(
        tab.panes.iter().find(|p| p.focused).unwrap().pane,
        pane("t2")
    );
    assert!(tab.zoomed);
    assert_eq!(tab.label, "remote logs");
    assert_eq!(tab.panes[1].label.as_deref(), Some("server"));
    own.fit("fake-core", &records[..1], &HashSet::new());
    let shown = layout(&records[..1], None, &own);
    let tab = &shown.workspaces[0].terminals[0];
    assert!(!tab.zoomed);
    assert!(tab.panes[0].focused);
    assert!(own.layouts.names.is_empty());
}

#[test]
fn saved_selection_is_projected_without_changing_tree_order() {
    let saved = sheprd_session::SavedTabs::parse(&serde_json::to_vec(&json!({"tabs": [
        {"id": 4, "scope": SCOPE, "workspace_id": "w1", "title": "build",
         "location": {"kind": "core_terminal", "panes": {"axis": "row", "first": "t1", "second": "t2"},
                      "active": "t2", "zoomed": "t2"}}
    ]})).unwrap()).unwrap();
    let records = records(&["t1", "t2"]);
    let shown = layout(&records, Some(&saved), &Store::default());
    let tab = &shown.workspaces[0].terminals[0];
    assert_eq!(panes(tab), [pane("t1"), pane("t2")]);
    assert!(!tab.panes[0].focused);
    assert!(tab.panes[1].focused);
    assert!(tab.zoomed);
}
