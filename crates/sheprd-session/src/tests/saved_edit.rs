use crate::{
    SavedTabs,
    panes::Axis,
    saved::edit::{self, Change, Error},
};
use serde_json::{Value, json};
use sheprd_backend::TerminalId;

fn file() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "tabs": [
            {"id": 1, "scope": "s", "workspace_id": "w", "title": "build", "zoom": 1.5,
             "location": {"kind": "core_terminal", "terminal": "t1", "active": "t1"}},
            {"id": 2, "scope": "s", "workspace_id": "w", "title": "web",
             "location": {"kind": "web", "url": "https://example.com"}},
            {"id": 3, "scope": "s", "workspace_id": "w", "title": "mirror",
             "location": {"kind": "core_terminal", "panes": "t9", "observe": true}},
            {"id": 1, "scope": "other", "workspace_id": "w", "title": "x",
             "location": {"kind": "core_terminal", "panes": "t1"}}
        ],
        "workspaces": [
            {"id": "w", "scope": "s", "label": "api", "cwd": "/code/api", "custom": false, "future": 7}
        ],
        "future": {"kept": true}
    }))
    .unwrap()
}

fn id(terminal: &str) -> TerminalId {
    TerminalId::new(terminal).unwrap()
}

fn value(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn a_split_keeps_every_field_it_does_not_name() {
    let at = id("t1");
    let change = Change::Split {
        at: &at,
        axis: Axis::Row,
        new: id("t2"),
    };
    let edited = edit::edit_tab(&file(), "s", 1, change).unwrap();
    let saved = SavedTabs::parse(&edited).unwrap();
    let tab = saved.core_tabs("s").next().unwrap();
    assert_eq!(tab.panes.terminals(), [&id("t1"), &id("t2")]);
    let edited = value(&edited);
    let location = &edited["tabs"][0]["location"];
    assert_eq!(location["active"], "t1");
    assert!(location.get("terminal").is_none(), "renamed to `panes`");
    assert_eq!(edited["tabs"][0]["zoom"], 1.5);
    assert_eq!(edited["future"]["kept"], true);
    assert_eq!(
        edited["tabs"][3]["location"]["panes"], "t1",
        "another scope's"
    );
    assert_eq!(edited["workspaces"][0]["future"], 7);
}

#[test]
fn zoom_toggles_and_a_split_shows_the_panes_again() {
    let (t1, t2) = (id("t1"), id("t2"));
    let split = Change::Split {
        at: &t1,
        axis: Axis::Column,
        new: t2.clone(),
    };
    let bytes = edit::edit_tab(&file(), "s", 1, split).unwrap();
    let zoomed = edit::edit_tab(&bytes, "s", 1, Change::Zoom(&t2)).unwrap();
    let saved = SavedTabs::parse(&zoomed).unwrap();
    assert_eq!(saved.tab_zoomed("s", 1), Some(&t2));
    let shown = edit::edit_tab(&zoomed, "s", 1, Change::Zoom(&t2)).unwrap();
    assert_eq!(SavedTabs::parse(&shown).unwrap().tab_zoomed("s", 1), None);
    let split = Change::Split {
        at: &t2,
        axis: Axis::Row,
        new: id("t3"),
    };
    let split = edit::edit_tab(&zoomed, "s", 1, split).unwrap();
    assert_eq!(SavedTabs::parse(&split).unwrap().tab_zoomed("s", 1), None);
}

#[test]
fn names_are_set_and_a_blank_one_gives_the_title_back() {
    let t1 = id("t1");
    let named = edit::edit_tab(&file(), "s", 1, Change::Rename(Some(" logs "))).unwrap();
    let named = edit::edit_tab(
        &named,
        "s",
        1,
        Change::RenamePane {
            at: &t1,
            name: Some("server"),
        },
    )
    .unwrap();
    let saved = SavedTabs::parse(&named).unwrap();
    assert_eq!(saved.tab_name("s", 1), Some("logs"));
    assert_eq!(saved.pane_names("s", 1).unwrap()["t1"], "server");
    let cleared = edit::edit_tab(&named, "s", 1, Change::Rename(Some("  "))).unwrap();
    let cleared = edit::edit_tab(
        &cleared,
        "s",
        1,
        Change::RenamePane {
            at: &t1,
            name: None,
        },
    )
    .unwrap();
    let location = &value(&cleared)["tabs"][0]["location"];
    assert!(location.get("name").is_none());
    assert!(location.get("names").is_none());
}

#[test]
fn close_drops_only_that_tab() {
    let closed = edit::edit_tab(&file(), "s", 1, Change::Close).unwrap();
    let tabs = value(&closed)["tabs"].as_array().unwrap().clone();
    assert_eq!(tabs.len(), 3);
    assert_eq!(tabs[2]["scope"], "other");
}

#[test]
fn what_is_not_a_typing_core_tab_is_refused() {
    let t9 = id("t9");
    assert!(matches!(
        edit::edit_tab(&file(), "s", 2, Change::Rename(Some("x"))),
        Err(Error::Tab)
    ));
    assert!(matches!(
        edit::edit_tab(&file(), "s", 3, Change::Zoom(&t9)),
        Err(Error::Watching)
    ));
    assert!(matches!(
        edit::edit_tab(&file(), "s", 8, Change::Close),
        Err(Error::Tab)
    ));
    assert!(matches!(
        edit::edit_tab(&file(), "s", 1, Change::Zoom(&t9)),
        Err(Error::Panes(crate::panes::Error::Missing(_)))
    ));
    assert!(matches!(
        edit::edit_tab(b"{", "s", 1, Change::Close),
        Err(Error::Format(_))
    ));
}

#[test]
fn a_workspace_is_renamed_as_the_user_named_it() {
    let renamed = edit::rename_workspace(&file(), "s", "w", "API").unwrap();
    let saved = SavedTabs::parse(&renamed).unwrap();
    let workspace = saved.workspaces("s").next().unwrap();
    assert_eq!(workspace.label, "API");
    assert!(workspace.custom);
    assert!(matches!(
        edit::rename_workspace(&file(), "other", "w", "API"),
        Err(Error::Workspace)
    ));
}

#[test]
fn zoom_gives_the_pane_the_keyboard_and_unzoom_works_from_any_pane() {
    let (t1, t2) = (id("t1"), id("t2"));
    let split = Change::Split {
        at: &t1,
        axis: Axis::Row,
        new: t2.clone(),
    };
    let bytes = edit::edit_tab(&file(), "s", 1, split).unwrap();
    let zoomed = edit::edit_tab(&bytes, "s", 1, Change::Zoom(&t2)).unwrap();
    assert_eq!(value(&zoomed)["tabs"][0]["location"]["active"], "t2");
    let shown = edit::edit_tab(&zoomed, "s", 1, Change::Zoom(&t1)).unwrap();
    assert_eq!(SavedTabs::parse(&shown).unwrap().tab_zoomed("s", 1), None);
}
