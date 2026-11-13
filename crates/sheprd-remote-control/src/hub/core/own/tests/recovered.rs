use super::*;
use sheprd_chat_protocol::TOKEN;
use sheprd_core_client::chat_recovery::REPLACES_LABEL;

const ENDPOINT: &str = "0123456789abcdef0123456789abcdef";

fn chat(address: &str, replaces: Option<&str>) -> Record {
    let mut labels = json!({TOKEN: ENDPOINT});
    if let Some(old) = replaces {
        labels[REPLACES_LABEL] = json!(format!("[\"{old}\"]"));
    }
    serde_json::from_value(json!({"address": address, "cols": 80, "rows": 24,
        "cwd": "/code/app", "state": "live", "pid": 1, "labels": labels}))
    .unwrap()
}

/// A phone names a lone chat without making a tab for it.
#[test]
fn a_lone_chat_keeps_its_phone_given_name_when_it_recovers() {
    let mut own = Store::default();
    own.fit("fake-core", &[chat("t1", None)], &HashSet::new());
    own.rename_pane(&id("t1"), Some("planner")).unwrap();
    assert!(own.layouts.tabs.is_empty());
    let recovered = [chat("t1", None), chat("t2", Some("t1"))];
    assert!(own.fit("fake-core", &recovered, &HashSet::new()));
    assert_eq!(
        own.layouts.names.get("t2").map(String::as_str),
        Some("planner")
    );
    assert!(!own.layouts.names.contains_key("t1"));
}

#[test]
fn a_recovered_selected_pane_stays_selected_in_a_phones_tab() {
    let mut own = Store::default();
    own.fit(
        "fake-core",
        &[chat("t1", None), record("t2")],
        &HashSet::new(),
    );
    own.split(&id("t2"), Axis::Row, id("t1")).unwrap();
    assert_eq!(own.layouts.tabs[0].active, Some(id("t1")));
    let recovered = [chat("t1", None), record("t2"), chat("t3", Some("t1"))];
    assert!(own.fit("fake-core", &recovered, &HashSet::new()));
    assert_eq!(own.layouts.tabs[0].active, Some(id("t3")));
}
