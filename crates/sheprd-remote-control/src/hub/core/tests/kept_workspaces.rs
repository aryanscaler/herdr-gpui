//! The workspaces the app keeps for Herdr 2 terminals, on the phone.

use super::*;

/// The folder a phone asks a new workspace for: absolute on this platform,
/// as a new workspace's label must be.
const FOLDER: &str = if cfg!(windows) {
    r"C:\code\api"
} else {
    "/code/api"
};

#[test]
fn kept_workspaces_lead_in_their_order_with_their_names_even_when_empty() {
    let saved = serde_json::from_value(json!({
        "workspaces": [
            {"id": "space-2", "scope": "local:/test.sock", "label": "api", "cwd": "/code/api"},
            {"id": "space-1", "scope": "local:/test.sock", "label": "web", "cwd": "/code/web"},
            {"id": "space-9", "scope": "local:/other.sock", "label": "elsewhere", "cwd": "/x"}
        ],
        "tabs": [
            {"id": 4, "scope": "local:/test.sock", "workspace_id": "space-1", "title": "build",
             "location": {"kind": "core_terminal", "panes": "t1"}}
        ]
    }))
    .unwrap();
    let records = ["t1", "t2"].map(|address| serde_json::from_value(record(address)).unwrap());
    let layout = projection::project(
        "fake-core",
        &records,
        Some(&saved),
        Some("local:/test.sock"),
    );
    let rows: Vec<(&str, &str, &str, usize)> = layout
        .workspaces
        .iter()
        .map(|row| {
            (
                row.id.as_str(),
                row.label.as_str(),
                row.cwd.as_str(),
                row.terminals.len(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("space-2", "api", "/code/api", 0),
            ("space-1", "web", "/code/web", 1),
            (UNLISTED, "Herdr 2", "/code/app", 1),
        ],
        "another host's workspace is left out"
    );
    assert_eq!(layout.workspaces[1].terminals[0].label, "build");
}

#[test]
fn kept_workspaces_lead_the_desktop_documents_spaces_too() {
    let dir = tempfile::tempdir().unwrap();
    let tabs = dir.path().join("browser-tabs.json");
    std::fs::write(
        &tabs,
        json!({"tabs": [], "workspaces": [
            {"id": "space-1", "scope": "local:/test.sock", "label": "api", "cwd": "/code/api"}
        ]})
        .to_string(),
    )
    .unwrap();
    let mut poller = crate::hub::core::poller::Poller::default();
    let doc = sheprd_core_client::wire::Doc {
        value: json!({"spaces": [{"id": "s", "folder": "/f", "name": "desk", "tabs": []}]}),
        version: 1,
    };
    poller.read_spaces("fake-core", Ok(doc)).unwrap();
    let layout = poller.project("fake-core", &[], Some(&tabs), Some("local:/test.sock"));
    let labels: Vec<&str> = layout
        .workspaces
        .iter()
        .map(|workspace| workspace.label.as_str())
        .collect();
    assert_eq!(labels, ["api", "desk"]);
}

#[test]
fn a_terminal_started_for_a_workspace_is_listed_there_before_it_has_a_tab() {
    let saved = serde_json::from_value(json!({
        "workspaces": [
            {"id": "space-2", "scope": "local:/test.sock", "label": "api", "cwd": "/code/api"}
        ],
        "tabs": []
    }))
    .unwrap();
    let mut phone = record("t1");
    phone["labels"] = json!({sheprd_session::WORKSPACE_LABEL: "space-2"});
    let mut made_up = record("t2");
    made_up["labels"] = json!({sheprd_session::WORKSPACE_LABEL: "core:space:x"});
    let records = [phone, made_up].map(|record| serde_json::from_value(record).unwrap());
    let layout = projection::project(
        "fake-core",
        &records,
        Some(&saved),
        Some("local:/test.sock"),
    );
    let rows: Vec<(&str, usize)> = layout
        .workspaces
        .iter()
        .map(|row| (row.id.as_str(), row.terminals.len()))
        .collect();
    // A label naming the daemon's own groups is not one a tab can be in.
    assert_eq!(rows, vec![("space-2", 1), (UNLISTED, 1)]);
}

#[test]
fn a_terminal_started_for_a_named_sessions_workspace_is_listed_under_its_phone_id() {
    let mut phone = record("t1");
    phone["labels"] = json!({
        sheprd_session::WORKSPACE_LABEL: "w1",
        sheprd_session::SESSION_LABEL: "dev",
    });
    let records = [serde_json::from_value(phone).unwrap()];
    let layout = projection::project("fake-core", &records, None, None);
    assert_eq!(layout.workspaces[0].id, "herdr:dev:w1");
}

#[test]
fn a_phones_new_workspace_is_listed_apart_and_named_for_its_folder() {
    let mut phone = record("t1");
    phone["labels"] = json!({sheprd_session::NEW_WORKSPACE_LABEL: FOLDER});
    let records = [serde_json::from_value(phone).unwrap()];
    let layout = projection::project("fake-core", &records, None, None);
    let row = &layout.workspaces[0];
    assert_eq!(row.id, new_workspace_id(FOLDER));
    assert_eq!((row.label.as_str(), row.cwd.as_str()), ("api", FOLDER));
    assert_eq!(row.terminals.len(), 1);
}

#[test]
fn a_phones_new_workspace_keeps_its_id_when_its_first_pane_closes() {
    let labelled = |address: &str| {
        let mut record = record(address);
        record["labels"] = json!({sheprd_session::NEW_WORKSPACE_LABEL: FOLDER});
        serde_json::from_value(record).unwrap()
    };
    let both = [labelled("t1"), labelled("t9")];
    let before = projection::project("fake-core", &both, None, None);
    // The first chat closes; the tab added after it stays.
    let after = projection::project("fake-core", &[labelled("t9")], None, None);
    assert_eq!(before.workspaces[0].id, after.workspaces[0].id);
    assert_eq!(after.workspaces[0].terminals.len(), 1);
    // Another folder is another workspace, and an id fits in a path.
    assert_ne!(new_workspace_id("/code/api"), new_workspace_id("/code/web"));
    assert!(!new_workspace_id("/code/api").contains('/'));
}
