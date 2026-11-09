use super::*;
use crate::tests::{connected, terminal};
use serde_json::{Value, json};
use std::thread;

/// Terminal `t1`, started as `program` with pid 100, with `front` in front
/// and Claude's last report.
fn claude_record(program: &str, front: i32, state: &str) -> Value {
    let mut value = terminal("t1");
    value["argv"] = json!([program]);
    value["pid"] = json!(100);
    value["foreground"] = json!({"pid": front, "name": "claude", "argv": ["claude"], "cwd": ""});
    value["status"] = json!({"state": state, "app": "claude-code"});
    value
}

fn record(value: Value) -> Terminal {
    serde_json::from_value(value).unwrap()
}

#[test]
fn the_agent_is_in_front_until_its_shell_is_back() {
    // Claude started from the terminal's shell, and still running.
    assert!(agent_in_front(
        &record(claude_record("zsh", 200, "idle")),
        "claude"
    ));
    // Claude quit: its report stays, but the shell has the terminal.
    let mut quit = claude_record("zsh", 100, "idle");
    quit["foreground"] = json!({"pid": 100, "name": "zsh", "argv": ["zsh"], "cwd": ""});
    assert!(!agent_in_front(&record(quit), "claude"));
    // Claude is the program the terminal started.
    assert!(agent_in_front(
        &record(claude_record("claude", 100, "idle")),
        "claude"
    ));
    // Another agent.
    assert!(!agent_in_front(
        &record(claude_record("zsh", 200, "idle")),
        "codex"
    ));
}

#[test]
fn an_idle_agent_is_typed_to_only_while_the_core_says_it_is_idle() {
    let (client, mut core) = connected();
    let call = thread::spawn(move || reply(&client, "t1", "claude", "go on"));
    let list = core.request();
    assert_eq!(list["method"], "terminal.list");
    core.answer(
        &list,
        json!({"terminals": [claude_record("zsh", 200, "idle")]}),
    );
    let send = core.request();
    assert_eq!(send["method"], "terminal.send");
    assert_eq!(
        send["params"],
        json!({"terminal": "t1", "text": "go on", "if": "idle"})
    );
    core.answer(&send, json!({"turn": 2, "started": true}));
    assert_eq!(call.join().unwrap().unwrap(), Replied::Sent);
}

#[test]
fn a_reply_to_an_agent_that_left_is_not_typed() {
    let (client, mut core) = connected();
    let call = thread::spawn(move || reply(&client, "t1", "claude", "rm -rf build"));
    let list = core.request();
    let mut quit = claude_record("zsh", 100, "idle");
    quit["foreground"] = json!({"pid": 100, "name": "zsh", "argv": ["zsh"], "cwd": ""});
    core.answer(&list, json!({"terminals": [quit]}));
    // No send follows.
    assert_eq!(call.join().unwrap().unwrap(), Replied::Gone);
}
