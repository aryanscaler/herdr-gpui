use super::*;
use crate::hub::core::tests_support_source;
use serde_json::{Value, json};
use std::path::Path;

const SCOPE: &str = "local:/test.sock";

fn saved_file(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("browser-tabs.json");
    std::fs::write(
        &path,
        json!({"tabs": [
            {"id": 4, "scope": SCOPE, "workspace_id": "w1", "title": "build",
             "location": {"kind": "core_terminal", "panes": {"axis": "row", "first": "t1", "second": "t2"}}}
        ], "workspaces": [
            {"id": "w1", "scope": SCOPE, "label": "api", "cwd": "/code/api", "custom": false}
        ]})
        .to_string(),
    )
    .unwrap();
    path
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn only_a_desktop_space_tab_keeps_its_layout_from_phones() {
    assert!(arrangeable("core:tab:4"));
    assert!(arrangeable("core:own:1"));
    assert!(arrangeable("w1:t1"));
    assert!(!arrangeable("core:app-tab:6162"));
}

#[test]
fn while_sheprd_is_closed_its_saved_tab_is_changed_in_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let source = tests_support_source(Some(path.clone()), Some(SCOPE));
    source.arrange_saved(4, "t2", &Op::Zoom).unwrap();
    assert_eq!(read(&path)["tabs"][0]["location"]["zoomed"], "t2");
    source
        .arrange_saved(4, "t2", &Op::RenameTab(Some("logs".into())))
        .unwrap();
    source
        .arrange_saved(4, "t1", &Op::RenamePane(Some("server".into())))
        .unwrap();
    let rename = Op::RenameWorkspace {
        workspace: "w1".into(),
        name: " API ".into(),
    };
    source.arrange_saved(0, "", &rename).unwrap();
    let saved = read(&path);
    assert_eq!(saved["tabs"][0]["location"]["name"], "logs");
    assert_eq!(saved["tabs"][0]["location"]["names"]["t1"], "server");
    assert_eq!(saved["workspaces"][0]["label"], "API");
    assert_eq!(saved["workspaces"][0]["custom"], true);
    source.arrange_saved(4, "t1", &Op::Close).unwrap();
    assert_eq!(read(&path)["tabs"], json!([]));
    assert!(matches!(
        source.arrange_saved(4, "t1", &Op::Zoom),
        Err(HubError::Core(super::super::Error::Arrange(Error::Edit(
            edit::Error::Tab
        ))))
    ));
}

#[test]
fn without_saved_tabs_there_is_nothing_to_change() {
    let source = tests_support_source(None, Some(SCOPE));
    assert!(matches!(
        source.arrange_saved(4, "t1", &Op::Zoom),
        Err(HubError::Core(super::super::Error::Arrange(Error::NoTabs)))
    ));
}

#[test]
fn a_file_sheprd_changed_meanwhile_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let result = edit_file(&path, |bytes| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, bytes).unwrap();
        Ok(b"{}".to_vec())
    });
    assert!(matches!(result, Err(Error::Changed)));
    assert_ne!(read(&path), json!({}));
}

#[test]
fn the_daemon_zooms_and_names_tabs_no_saved_tab_holds() {
    let source = tests_support_source(None, None);
    source.arrange_own("t1", &Op::Zoom).unwrap();
    source
        .arrange_own("t1", &Op::RenameTab(Some("logs".into())))
        .unwrap();
    source
        .arrange_own("t5", &Op::RenamePane(Some("claude".into())))
        .unwrap();
    source.name_group("core:terminals", "Box").unwrap();
    let records: Vec<Terminal> = ["t1", "t5"]
        .map(|address| {
            serde_json::from_value(json!({"address": address, "cols": 80, "rows": 24,
                "cwd": "/code/app", "state": "live", "pid": 1}))
            .unwrap()
        })
        .to_vec();
    let mut layout = projection::project("fake-core", &records, None, None);
    lock(&source.own)
        .layouts
        .apply(&mut layout, "fake-core", &records);
    let workspace = &layout.workspaces[0];
    assert_eq!(workspace.label, "Box");
    assert_eq!(workspace.terminals[0].label, "logs");
    assert!(workspace.terminals[0].zoomed);
    assert!(workspace.terminals[0].tab.starts_with(OWN_TAB));
    assert_eq!(
        workspace.terminals[1].panes[0].label.as_deref(),
        Some("claude")
    );
    assert!(matches!(
        source.arrange_own(
            "t1",
            &Op::RenameWorkspace {
                workspace: "x".into(),
                name: "y".into()
            }
        ),
        Err(HubError::Core(super::super::Error::Unsupported))
    ));
}

#[test]
fn a_starting_or_running_sheprd_holding_its_tabs_keeps_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let before = std::fs::read(&path).unwrap();
    let sheprd = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(sheprd_session::saved::lock_path(&path))
        .unwrap();
    sheprd.lock().unwrap();
    let source = tests_support_source(Some(path.clone()), Some(SCOPE));
    assert!(matches!(
        source.arrange_saved(4, "t2", &Op::Zoom),
        Err(HubError::Core(super::super::Error::Arrange(
            Error::SheprdOpen
        )))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(sheprd);
    source.arrange_saved(4, "t2", &Op::Zoom).unwrap();
}
