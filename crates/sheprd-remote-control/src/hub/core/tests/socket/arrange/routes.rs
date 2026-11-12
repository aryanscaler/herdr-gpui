//! Both phone clients call these routes. No Herdr 1 peer or desktop answers.
use super::*;
use crate::web::tests::{ORIGIN, send, signed_in, state_with};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};

#[tokio::test]
async fn shared_phone_routes_edit_saved_tabs_with_a_stale_desktop_heartbeat() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("browser-tabs.json");
    std::fs::write(&path, json!({"tabs": [{
        "id": 4, "scope": "local:/test.sock", "workspace_id": "w1", "title": "shell",
        "location": {"kind": "core_terminal", "panes": "t1", "active": "t1"}
    }], "workspaces": [{"id": "w1", "scope": "local:/test.sock", "label": "app", "cwd": "/code/app"}]}).to_string()).unwrap();
    let (mut source, fake) = fake_with_doc(false, Value::Null, vec![record("t1"), record("t9")]);
    let editable = Arc::get_mut(&mut source).unwrap();
    editable.tabs = Some(path.clone());
    editable.scope = Some("local:/test.sock".into());
    let layout = source.poll().unwrap();
    let mut hub = Hub::offline_in(layout, None, None);
    hub.core = Some(source.clone());
    let state = state_with(dir.path(), Arc::new(hub));
    state.changes.seen();
    let (_, token) = signed_in(&state, 7);
    let pane = id("t1");
    for (route, body) in [
        (
            format!("/api/term/{pane}/action"),
            json!({"action": "split_right"}),
        ),
        (
            format!("/api/term/{pane}/action"),
            json!({"action": "zoom"}),
        ),
        (
            format!("/api/term/{pane}/rename"),
            json!({"label": "phone tab"}),
        ),
        (
            "/api/v2/workspaces/w1/rename".into(),
            json!({"label": "phone workspace"}),
        ),
    ] {
        let request = Request::post(&route)
            .header("Authorization", format!("Bearer {token}"))
            .header("Origin", ORIGIN)
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _, body) = tokio::time::timeout(Duration::from_secs(1), send(&state, request))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK, "{route}: {body}");
    }
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let location = &saved["tabs"][0]["location"];
    assert_eq!(location["panes"]["second"], "t9");
    assert_eq!(location["active"], "t1");
    assert_eq!(location["zoomed"], "t1");
    assert_eq!(location["name"], "phone tab");
    assert_eq!(saved["workspaces"][0]["label"], "phone workspace");
    assert_eq!(run_request(&fake)["method"], "terminal.run");
    assert!(state.changes.take(Duration::ZERO).1.is_empty());
    drop((state, source));
    fake.server.join().unwrap();
}
