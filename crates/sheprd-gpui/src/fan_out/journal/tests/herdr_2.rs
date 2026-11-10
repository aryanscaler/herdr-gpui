//! Herdr 2 lanes in the journal: their checkouts are known by folder, their
//! agents by herdr-core terminal, and journals from before Herdr 2 still read.

use super::*;

fn kept_checkout() -> Checkout {
    Checkout {
        endpoint_id: "local".into(),
        host: origin().host,
        worktree: work::Worktree {
            path: "/tmp/lane-kept".into(),
            opened: work::Opened::Herdr2,
        },
        terminal: None,
    }
}

#[test]
fn a_herdr_2_lane_keeps_its_folder_and_agent_terminal() {
    let mut origin = origin();
    origin.checkout = Some("/tmp/repo".into());
    let mut record = Record::new(1, &origin, "fix the bug", &lanes());
    assert_eq!(record.origin.checkout.as_deref(), Some("/tmp/repo"));
    record.advance(&lane(0, Progress::CreatingWorktree));
    assert!(record.advance(&lane(0, Progress::Created(Box::new(kept_checkout())))));
    record.advance(&lane(0, Progress::StartingAgent));
    assert!(record.advance(&lane(0, Progress::Terminal("t9".into()))));
    assert!(
        !record.advance(&lane(0, Progress::Terminal("t9".into()))),
        "the same terminal changes nothing"
    );
    assert_eq!(record.lanes[0].step, Checkpoint::StartingAgent);
    let saved = record.lanes[0].checkout.clone().unwrap();
    assert!(saved.herdr_2);
    assert_eq!(saved.terminal.as_deref(), Some("t9"));
    let restored = saved.checkout(origin.host.clone());
    assert_eq!(restored.worktree, kept_checkout().worktree);
    assert_eq!(restored.terminal.as_deref(), Some("t9"));
    assert_eq!(restored.workspace_id(), None);
    assert!(record.valid());
}

#[test]
fn a_herdr_1_lane_takes_no_terminal_and_saves_as_before() {
    let mut record = record(1);
    record.advance(&lane(0, Progress::CreatingWorktree));
    record.advance(&lane(0, Progress::Created(Box::new(checkout(1)))));
    assert!(!record.advance(&lane(0, Progress::Terminal("t9".into()))));
    let json = serde_json::to_value(&record).unwrap();
    let saved = &json["lanes"][0]["checkout"];
    assert!(saved.get("herdr_2").is_none(), "{saved}");
    assert!(saved.get("terminal").is_none(), "{saved}");
    assert!(json["origin"].get("checkout").is_none());
}

#[test]
fn a_journal_from_before_herdr_2_reads_as_herdr_1() {
    let json = serde_json::json!({"records": [{
        "id": 4,
        "origin": {
            "endpoint_id": "local", "host": "h", "endpoint_label": "This Mac",
            "workspace_id": "w0", "repo_key": "/tmp/repo/.git",
            "repo_label": "repo", "base": "HEAD"
        },
        "commit": "c0ffee",
        "prompt": "fix the bug",
        "lanes": [{
            "kind": "claude", "branch": "b", "agent": "a", "step": "created",
            "checkout": {
                "endpoint_id": "local", "host": "h", "workspace_id": "w1",
                "path": "/tmp/lane", "pane_id": "p1"
            }
        }]
    }]});
    let parsed = parse(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].origin.checkout, None);
    let checkout = parsed[0].lanes[0]
        .checkout
        .as_ref()
        .unwrap()
        .checkout(origin().host);
    assert_eq!(checkout.workspace_id(), Some("w1"));
    assert_eq!(
        checkout.worktree.opened,
        work::Opened::Herdr1 {
            workspace_id: "w1".into(),
            pane_id: "p1".into(),
        }
    );
}
