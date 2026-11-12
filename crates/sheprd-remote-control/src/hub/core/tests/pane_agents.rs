//! A Herdr 2 split keeps each pane's agent, and a chat pane leaves them.

use super::*;
use sheprd_client::protocol::AgentStatus;

#[test]
fn a_split_keeps_every_panes_agent_and_a_chat_is_not_one() {
    let saved = serde_json::from_value(json!({"tabs": [
        {"id": 4, "scope": "local:/test.sock", "workspace_id": "w1", "title": "build",
         "location": {"kind": "core_terminal", "panes": {"axis": "row",
            "first": "t1", "second": {"axis": "column", "first": "t2", "second": "t3"}}}}
    ]}))
    .unwrap();
    let mut done = record("t2");
    done["status"]["state"] = json!("done");
    let mut chat = record("t3");
    chat["labels"] = json!({ sheprd_chat_protocol::TOKEN: "0123456789abcdef0123456789abcdef" });
    chat["foreground"] = json!({"pid":42,"name":"sheprd-acp","argv":["sheprd-acp"]});
    let records: Vec<Terminal> = [record("t1"), done, chat]
        .into_iter()
        .map(|record| serde_json::from_value(record).unwrap())
        .collect();
    let mut core = projection::project(
        "fake-core",
        &records,
        Some(&saved),
        Some("local:/test.sock"),
    );
    let tab = &core.workspace("w1").unwrap().terminals[0];
    let agents: Vec<_> = tab
        .agents
        .iter()
        .map(|agent| (agent.pane.clone(), agent.status))
        .collect();
    assert_eq!(
        agents,
        [
            (id("t1"), AgentStatus::Working),
            (id("t2"), AgentStatus::Done),
            (id("t3"), AgentStatus::Working),
        ]
    );
    chats::lift(&mut core, "fake-core", &records);
    let workspace = core.workspace("w1").unwrap();
    assert_eq!(workspace.chats.len(), 1);
    let panes: Vec<_> = workspace.terminals[0]
        .agents
        .iter()
        .map(|agent| agent.pane.clone())
        .collect();
    assert_eq!(panes, [id("t1"), id("t2")], "the chat alerts as a chat");
}
