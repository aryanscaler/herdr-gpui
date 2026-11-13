#![allow(clippy::unwrap_used)]
use crate::saved::edit;
use serde_json::{Value, json};
use sheprd_backend::TerminalId;
use std::collections::BTreeMap;

fn map() -> BTreeMap<TerminalId, TerminalId> {
    BTreeMap::from([(
        TerminalId::new("t1").unwrap(),
        TerminalId::new("t9").unwrap(),
    )])
}

#[test]
fn remapping_a_saved_file_moves_place_name_focus_and_zoom_and_keeps_the_rest() {
    let file = json!({"tabs": [
        {"id": 1, "scope": "s", "workspace_id": "w", "title": "a", "future": 3,
         "location": {"kind": "core_terminal", "panes": {"axis": "row", "first": "t1", "second": "t2"},
                      "names": {"t1": "chat", "t2": "logs"}, "active": "t1", "zoomed": "t1", "extra": true}},
        {"id": 2, "scope": "s", "workspace_id": "w", "title": "b",
         "location": {"kind": "core_terminal", "terminal": "t1"}},
        {"id": 3, "scope": "other", "workspace_id": "w", "title": "c",
         "location": {"kind": "core_terminal", "panes": "t1"}},
        {"id": 4, "scope": "s", "workspace_id": "w", "title": "d",
         "location": {"kind": "web", "url": "https://example.com"}}
    ], "future": {"kept": true}});
    let bytes = edit::remap(&serde_json::to_vec(&file).unwrap(), "s", &map()).unwrap();
    let saved: Value = serde_json::from_slice(&bytes).unwrap();
    let first = &saved["tabs"][0]["location"];
    assert_eq!(first["panes"]["first"], "t9");
    assert_eq!(first["panes"]["second"], "t2");
    assert_eq!(first["names"], json!({"t9": "chat", "t2": "logs"}));
    assert_eq!(first["active"], "t9");
    assert_eq!(first["zoomed"], "t9");
    assert_eq!(first["extra"], true);
    assert_eq!(saved["tabs"][0]["future"], 3);
    assert_eq!(saved["tabs"][1]["location"]["terminal"], "t9");
    // Another host's tab names another core's terminal t1.
    assert_eq!(saved["tabs"][2]["location"]["panes"], "t1");
    assert_eq!(saved["tabs"][3], file["tabs"][3]);
    assert_eq!(saved["future"], json!({"kept": true}));
}

/// The selected second pane recovers: the selection must follow it, or the
/// phone falls back to the first pane.
#[test]
fn a_recovered_selected_pane_stays_selected() {
    let file = json!({"tabs": [
        {"id": 1, "scope": "s", "workspace_id": "w", "title": "a",
         "location": {"kind": "core_terminal", "panes": {"axis": "row", "first": "t2", "second": "t1"},
                      "active": "t1"}}
    ]});
    let mut saved = crate::SavedTabs::parse(&serde_json::to_vec(&file).unwrap()).unwrap();
    saved.remap("s", &map());
    assert_eq!(saved.tab_active("s", 1).map(TerminalId::as_str), Some("t9"));
}
