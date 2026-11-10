//! A phone splits a Herdr 2 tab no saved tab holds: the daemon starts the
//! shell beside it, labelled for its workspace, and lists both in one tab.

use super::*;
use crate::control::layout::Op;

fn run_request(fake: &Fake) -> Value {
    let sent = fake.inputs.recv_timeout(Duration::from_secs(2)).unwrap();
    serde_json::from_slice(&sent).unwrap()
}

#[test]
fn a_split_starts_a_shell_beside_the_pane_and_lists_one_tab() {
    let mut labelled = record("t1");
    labelled["labels"] = json!({"sheprd.workspace": "w1", "other": "x"});
    let (source, fake) = fake_with_doc(false, Value::Null, vec![labelled, record("t9")]);
    let before = source.poll().unwrap();
    assert_eq!(before.workspace("w1").unwrap().terminals.len(), 1);

    let created = source.arrange_own("t1", &Op::SplitDown).unwrap();
    assert_eq!(created.as_deref(), Some("t9"));
    let run = run_request(&fake);
    assert_eq!(run["method"], "terminal.run");
    assert_eq!(run["params"]["cwd"], "/code/app");
    assert_eq!(
        run["params"]["labels"],
        json!({"sheprd.workspace": "w1"}),
        "only where it belongs is carried over"
    );

    let after = source.poll().unwrap();
    let (workspace, tab) = after.terminal(&id("t9")).unwrap();
    assert_eq!(workspace.id, "w1");
    assert!(tab.tab.starts_with(own::OWN_TAB));
    let panes: Vec<&str> = tab.panes.iter().map(|pane| pane.pane.as_str()).collect();
    assert_eq!(panes, [id("t1"), id("t9")]);
    drop(source);
    fake.server.join().unwrap();
}
