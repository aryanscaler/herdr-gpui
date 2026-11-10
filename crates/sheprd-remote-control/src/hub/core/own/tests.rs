use super::*;
use serde_json::json;

const SCOPE: &str = "local:/test.sock";

fn record(address: &str) -> Record {
    serde_json::from_value(json!({"address": address, "cols": 80, "rows": 24,
        "cwd": "/code/app", "state": "live", "pid": 1}))
    .unwrap()
}

fn records(addresses: &[&str]) -> Vec<Record> {
    addresses.iter().map(|address| record(address)).collect()
}

fn id(address: &str) -> TerminalId {
    TerminalId::new(address).unwrap()
}

fn pane(address: &str) -> String {
    pane_id("fake-core", address)
}

fn panes(terminal: &crate::hub::Terminal) -> Vec<String> {
    terminal
        .panes
        .iter()
        .map(|pane| pane.pane.clone())
        .collect()
}

fn layout(records: &[Record], saved: Option<&sheprd_session::SavedTabs>, own: &Store) -> Layout {
    let mut layout = projection::project("fake-core", records, saved, Some(SCOPE));
    own.layouts.apply(&mut layout, "fake-core", records);
    layout
}

#[test]
fn a_split_joins_lone_terminals_into_one_tab_with_its_names_and_zoom() {
    let records = records(&["t1", "t2", "t3"]);
    let mut own = Store::default();
    assert!(
        !own.fit("fake-core", &records, &HashSet::new()),
        "a first core changes nothing saved"
    );
    own.split(&id("t1"), Axis::Row, id("t2")).unwrap();
    own.zoom(&id("t2")).unwrap();
    own.rename_tab(&id("t1"), Some(" logs ")).unwrap();
    own.rename_pane(&id("t3"), Some("server")).unwrap();
    own.rename_workspace(crate::hub::core::UNLISTED, "Box")
        .unwrap();

    let layout = layout(&records, None, &own);
    let workspace = layout.workspace(crate::hub::core::UNLISTED).unwrap();
    assert_eq!(workspace.label, "Box");
    assert_eq!(workspace.terminals.len(), 2);
    let tab = &workspace.terminals[0];
    assert!(tab.tab.starts_with(OWN_TAB));
    assert_eq!(panes(tab), [pane("t1"), pane("t2")]);
    assert_eq!(tab.label, "logs");
    assert!(tab.zoomed);
    let lone = &workspace.terminals[1];
    assert_eq!(lone.panes[0].label.as_deref(), Some("server"));
    assert!(!lone.zoomed);

    own.zoom(&id("t2")).unwrap();
    assert!(!self::layout(&records, None, &own).workspaces[0].terminals[0].zoomed);
}

#[test]
fn a_terminal_one_of_sheprds_tabs_holds_stays_there() {
    let records = records(&["t1", "t2"]);
    let mut own = Store::default();
    own.fit("fake-core", &records, &HashSet::new());
    own.split(&id("t1"), Axis::Column, id("t2")).unwrap();
    let saved = serde_json::from_value(json!({"tabs": [
        {"id": 4, "scope": SCOPE, "workspace_id": "w1", "title": "build",
         "location": {"kind": "core_terminal", "panes": "t2"}}
    ]}))
    .unwrap();
    let layout = layout(&records, Some(&saved), &own);
    let saved_tab = &layout.workspace("w1").unwrap().terminals[0];
    assert_eq!(panes(saved_tab), [pane("t2")]);
    let (_, own_tab) = layout.terminal(&pane("t1")).unwrap();
    assert_eq!(panes(own_tab), [pane("t1")]);
}

#[test]
fn ended_terminals_leave_and_another_core_starts_over() {
    let mut own = Store::default();
    own.fit("fake-core", &records(&["t1", "t2", "t3"]), &HashSet::new());
    own.split(&id("t1"), Axis::Row, id("t2")).unwrap();
    own.split(&id("t2"), Axis::Row, id("t3")).unwrap();
    own.rename_pane(&id("t3"), Some("server")).unwrap();
    assert!(!own.fit("fake-core", &records(&["t1", "t2", "t3"]), &HashSet::new()));
    assert!(own.fit("fake-core", &records(&["t1", "t2"]), &HashSet::new()));
    assert_eq!(own.layouts.tabs[0].panes.count(), 2);
    assert!(own.layouts.names.is_empty());
    // A tab of one unnamed pane is no layout at all.
    assert!(own.fit("fake-core", &records(&["t1"]), &HashSet::new()));
    assert!(own.layouts.tabs.is_empty());

    own.rename_tab(&id("t1"), Some("kept")).unwrap();
    assert!(own.fit("other-core", &records(&["t1"]), &HashSet::new()));
    assert!(own.layouts.tabs.is_empty());
    assert_eq!(own.layouts.core, "other-core");
}

#[test]
fn layouts_survive_a_restart_in_the_state_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut own = Store::load(dir.path());
    own.fit("fake-core", &records(&["t1", "t2"]), &HashSet::new());
    own.split(&id("t1"), Axis::Row, id("t2")).unwrap();
    own.rename_workspace("core:new:ab", "api").unwrap();
    own.save().unwrap();
    let loaded = Store::load(dir.path());
    assert_eq!(loaded.layouts, own.layouts);

    std::fs::write(dir.path().join(FILE), b"{").unwrap();
    assert_eq!(Store::load(dir.path()).layouts, Layouts::default());
}

#[test]
fn names_are_bounded_and_a_blank_one_clears() {
    let mut own = Store::default();
    own.rename_pane(&id("t1"), Some(&"é".repeat(MAX_NAME)))
        .unwrap();
    assert!(own.layouts.names["t1"].len() <= MAX_NAME);
    own.rename_pane(&id("t1"), Some("  ")).unwrap();
    assert!(own.layouts.names.is_empty());
    assert!(matches!(
        own.rename_workspace("core:terminals", " "),
        Err(Error::Name)
    ));
}

#[test]
fn a_split_shell_a_listing_missed_stays_in_its_tab() {
    let mut own = Store::default();
    own.fit("fake-core", &records(&["t1"]), &HashSet::new());
    own.split(&id("t1"), Axis::Row, id("t2")).unwrap();
    // Listed just before the shell started.
    assert!(!own.fit("fake-core", &records(&["t1"]), &HashSet::new()));
    assert_eq!(own.layouts.tabs[0].panes.count(), 2);
    assert!(!own.fit("fake-core", &records(&["t1", "t2"]), &HashSet::new()));
    assert!(own.fresh.is_empty(), "listed, so no longer fresh");
    assert!(
        own.fit("fake-core", &records(&["t1"]), &HashSet::new()),
        "then it ended"
    );
}

#[test]
fn a_terminal_sheprd_takes_in_leaves_the_daemons_tab_and_its_room() {
    let mut own = Store::default();
    let all = records(&["t1", "t2", "t3"]);
    own.fit("fake-core", &all, &HashSet::new());
    own.split(&id("t1"), Axis::Row, id("t2")).unwrap();
    own.split(&id("t2"), Axis::Row, id("t3")).unwrap();
    let held = HashSet::from(["t3".to_owned()]);
    assert!(own.fit("fake-core", &all, &held));
    let terminals = own.layouts.tabs[0].panes.terminals();
    assert_eq!(terminals, [&id("t1"), &id("t2")]);
}

#[test]
fn an_empty_group_a_phone_named_shows_its_name_only_when_alone() {
    let mut own = Store::default();
    own.rename_workspace(crate::hub::core::UNLISTED, "Box")
        .unwrap();
    let mut core = projection::project("fake-core", &[], None, Some(SCOPE));
    own.layouts.apply(&mut core, "fake-core", &[]);
    let alone = crate::hub::core::merge(&Layout::default(), &core);
    assert_eq!(alone.workspaces.len(), 1);
    assert_eq!(alone.workspaces[0].label, "Box");

    let mut daemon = Layout {
        connected: true,
        ..Layout::default()
    };
    daemon.workspaces.push(alone.workspaces[0].clone());
    daemon.workspaces[0].id = "w1".into();
    let beside = crate::hub::core::merge(&daemon, &core);
    assert!(beside.workspace(crate::hub::core::UNLISTED).is_none());
}

#[test]
fn unzoom_from_another_pane_shows_the_split_again() {
    let mut own = Store::default();
    own.fit("fake-core", &records(&["t1", "t2"]), &HashSet::new());
    own.split(&id("t1"), Axis::Row, id("t2")).unwrap();
    own.zoom(&id("t1")).unwrap();
    own.zoom(&id("t2")).unwrap();
    assert_eq!(own.layouts.tabs[0].zoomed, None);
}
