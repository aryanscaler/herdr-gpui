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
    // The shell became Claude (`exec claude`): its pid, Claude in front.
    let mut exec = claude_record("zsh", 100, "idle");
    exec["foreground"] = json!({"pid": 100, "name": "claude", "argv": ["claude"], "cwd": ""});
    assert!(agent_in_front(&record(exec.clone()), "claude"));
    // Claude quit and the shell became Codex, before Codex reported.
    let mut codex = exec;
    codex["foreground"] = json!({"pid": 100, "name": "codex", "argv": ["codex"], "cwd": ""});
    assert!(!agent_in_front(&record(codex), "claude"));
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

fn until(deadline: std::time::Instant) -> (std::sync::atomic::AtomicBool, std::time::Instant) {
    (std::sync::atomic::AtomicBool::new(false), deadline)
}

#[test]
fn a_note_waits_for_the_turn_to_end_then_goes_to_the_idle_agent() {
    let (client, mut core) = connected();
    let call = thread::spawn(move || {
        let (stop, deadline) =
            until(std::time::Instant::now() + std::time::Duration::from_secs(60));
        when_free(
            &client,
            "t1",
            "claude",
            "see note",
            Until {
                deadline,
                stop: &stop,
            },
        )
    });
    let list = core.request();
    core.answer(
        &list,
        json!({"terminals": [claude_record("zsh", 200, "working")]}),
    );
    let wait = core.request();
    assert_eq!(wait["method"], "terminal.wait");
    core.answer(
        &wait,
        json!({"ended": true, "reason": "status", "status": "idle", "turn": 3}),
    );
    let list = core.request();
    core.answer(
        &list,
        json!({"terminals": [claude_record("zsh", 200, "idle")]}),
    );
    let send = core.request();
    assert_eq!(
        send["params"],
        json!({"terminal": "t1", "text": "see note", "if": "idle"})
    );
    core.answer(&send, json!({"turn": 4, "started": true}));
    assert_eq!(call.join().unwrap().unwrap(), Replied::Sent);
}

#[test]
fn past_the_deadline_a_note_goes_in_whatever_the_agent_does() {
    let (client, mut core) = connected();
    let call = thread::spawn(move || {
        let (stop, deadline) = until(std::time::Instant::now());
        when_free(
            &client,
            "t1",
            "claude",
            "see note",
            Until {
                deadline,
                stop: &stop,
            },
        )
    });
    let list = core.request();
    core.answer(
        &list,
        json!({"terminals": [claude_record("zsh", 200, "working")]}),
    );
    let send = core.request();
    assert_eq!(send["method"], "terminal.send");
    assert_eq!(
        send["params"],
        json!({"terminal": "t1", "text": "see note"})
    );
    core.answer(&send, json!({"turn": 4, "started": true}));
    assert_eq!(call.join().unwrap().unwrap(), Replied::Sent);
}

#[test]
fn a_note_to_an_agent_that_left_is_not_typed() {
    let (client, mut core) = connected();
    let call = thread::spawn(move || {
        let (stop, deadline) =
            until(std::time::Instant::now() + std::time::Duration::from_secs(60));
        when_free(
            &client,
            "t1",
            "codex",
            "see note",
            Until {
                deadline,
                stop: &stop,
            },
        )
    });
    let list = core.request();
    core.answer(
        &list,
        json!({"terminals": [claude_record("zsh", 200, "idle")]}),
    );
    assert_eq!(call.join().unwrap().unwrap(), Replied::Gone);
}
