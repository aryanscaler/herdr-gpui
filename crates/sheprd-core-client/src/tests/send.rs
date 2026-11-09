//! Typing a message into a terminal through the core.

use super::connected;
use crate::{
    Error, Method,
    frame::Kind,
    wire::{BLOCKED, SendText},
};
use serde_json::json;
use std::thread;

#[test]
fn a_send_carries_the_terminal_and_text() {
    let (client, mut core) = connected();
    let call = thread::spawn(move || {
        client.send(&SendText {
            terminal: "t3",
            text: "go on",
            ..SendText::default()
        })
    });
    let request = core.request();
    assert_eq!(request["method"], "terminal.send");
    assert_eq!(
        request["params"],
        json!({"terminal": "t3", "text": "go on"})
    );
    core.answer(
        &request,
        json!({"turn": 4, "started": true, "status": "idle"}),
    );
    let sent = call.join().unwrap().unwrap();
    assert_eq!((sent.turn, sent.started), (4, Some(true)));
}

#[test]
fn a_blocked_program_is_refused_by_kind() {
    let (client, mut core) = connected();
    let call = thread::spawn(move || {
        client.send(&SendText {
            terminal: "t3",
            text: "yes",
            ..SendText::default()
        })
    });
    let request = core.request();
    let message = json!({"jsonrpc": "2.0", "id": request["id"],
        "error": {"code": -32000, "message": "t3 is blocked", "data": {"kind": "blocked"}}});
    core.send(0, Kind::Data, &serde_json::to_vec(&message).unwrap());
    assert!(matches!(
        call.join().unwrap(),
        Err(Error::Refused { method: Method::TerminalSend, kind: Some(kind), .. }) if kind == BLOCKED
    ));
}
