//! Labelling a running terminal through the core.

use super::{connected, terminal};
use serde_json::json;
use std::{collections::BTreeMap, thread};

#[test]
fn a_label_carries_the_terminal_and_its_labels() {
    let (client, mut core) = connected();
    let call = thread::spawn(move || {
        client.label("t3", &BTreeMap::from([("herdr.acp", "abc"), ("gone", "")]))
    });
    let request = core.request();
    assert_eq!(request["method"], "terminal.label");
    assert_eq!(
        request["params"],
        json!({"terminal": "t3", "labels": {"herdr.acp": "abc", "gone": ""}})
    );
    core.answer(&request, terminal("t3"));
    assert_eq!(call.join().unwrap().unwrap().address, "t3");
}
