use super::*;
use sheprd_session::{CoreWorkspace, Session, Worktree};
use std::sync::Arc;

fn kept() -> Arc<Session> {
    Arc::new(Session::core(
        [CoreWorkspace {
            branch: Some("topic"),
            custom_label: false,
            worktree: Some(Worktree {
                key: "/repo/.git".into(),
                label: "repo".into(),
                linked: true,
            }),
            cwd: "/worktrees/topic",
            id: "kept",
            label: "topic",
        }],
        [],
        None,
        |_| None,
    ))
}

#[test]
fn core_only_repositories_need_no_herdr_1_snapshot() {
    let sessions = Sessions {
        core: Some(kept()),
        core_answers: true,
        herdr_1: None,
    };
    let found: Vec<_> = repositories("local", &sessions).collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, repo_key("local", Some("/repo/.git"), "unused"));
    assert_eq!(found[0].1.id, "kept");
    assert_eq!(found[0].2.checkout, "/worktrees/topic");
}

#[test]
fn mixed_sessions_preserve_order_and_share_keys_across_worktrees() {
    let mut herdr_1 = (*kept()).clone();
    herdr_1.workspaces[0].id = "h1".into();
    herdr_1.workspaces[0].cwd = "/repo".into();
    herdr_1.workspaces[0].keeper = sheprd_session::Keeper::Herdr1;
    let sessions = Sessions {
        core: Some(kept()),
        core_answers: true,
        herdr_1: Some(Arc::new(herdr_1)),
    };
    let found: Vec<_> = repositories("local", &sessions).collect();
    assert_eq!(
        found
            .iter()
            .map(|(_, w, _)| w.id.as_str())
            .collect::<Vec<_>>(),
        ["h1", "kept"]
    );
    assert_eq!(found[0].0, found[1].0);
    assert_ne!(found[0].0, repo_key("remote", Some("/repo/.git"), "/repo"));
}
