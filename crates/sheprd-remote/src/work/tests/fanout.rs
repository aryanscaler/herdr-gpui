use super::*;
use crate::work::{
    fanout::checkouts,
    owned_agent::{self, Owner},
};

#[test]
fn creation_receipt_recovers_without_creating_or_removing_someone_elses_checkout() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let repo = fixture.repo.to_str().unwrap();
    let new = NewWorktree {
        branch: "fan-out/lane-a",
        base: "HEAD",
        label: None,
    };
    let tree = checkouts::plan(&host, repo, new.branch, &never()).unwrap();
    assert!(!Path::new(&tree.path).exists());
    assert!(checkouts::verify(&host, &tree, new.branch, "lane-a", &never()).is_err());
    checkouts::create(&host, repo, &tree, &new, "lane-a", &never()).unwrap();
    checkouts::verify(&host, &tree, new.branch, "lane-a", &never()).unwrap();
    assert!(checkouts::create(&host, repo, &tree, &new, "other", &never()).is_err());
    assert!(checkouts::discard(&host, &tree, new.branch, "other", &never()).is_err());
    fs::write(Path::new(&tree.path).join("retained"), "work").unwrap();
    git(Path::new(&tree.path), &["switch", "-c", "unrelated"]);
    assert!(checkouts::discard(&host, &tree, new.branch, "lane-a", &never()).is_err());
    assert!(Path::new(&tree.path).join("retained").exists());
    git(Path::new(&tree.path), &["switch", new.branch]);
    checkouts::discard(&host, &tree, new.branch, "lane-a", &never()).unwrap();
    assert!(!Path::new(&tree.path).exists());
    assert_eq!(
        git(&fixture.repo, &["branch", "--list", new.branch]),
        new.branch
    );
}

#[test]
fn interrupted_creation_without_receipt_never_claims_a_matching_branch() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let repo = fixture.repo.to_str().unwrap();
    let new = NewWorktree {
        branch: "fan-out/lane-a",
        base: "HEAD",
        label: None,
    };
    let tree = checkouts::plan(&host, repo, new.branch, &never()).unwrap();
    git(
        &fixture.repo,
        &["worktree", "add", "-b", new.branch, &tree.path],
    );
    assert!(checkouts::verify(&host, &tree, new.branch, "lane-a", &never()).is_err());
    assert!(checkouts::discard(&host, &tree, new.branch, "lane-a", &never()).is_err());
    assert!(Path::new(&tree.path).exists());
}

#[test]
fn atomic_agent_run_labels_ownership_and_never_types_a_second_start() {
    let fixture = Fixture::new();
    fixture.program("herdr-core", "echo '{\"address\":\"t9\"}'");
    let tree = Worktree {
        path: "/work/lane".into(),
        opened: Opened::Herdr2,
    };
    let agent = Agent {
        kind: AgentKind::Claude,
        name: "lane-a",
        model: None,
        prompt: None,
    };
    owned_agent::start(
        &fixture.host(),
        &tree,
        &agent,
        Owner {
            key: "sheprd.fanout.lane",
            value: agent.name,
        },
        &never(),
    )
    .unwrap();
    let calls = fixture.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].contains("--label sheprd.fanout.lane=lane-a"));
    assert!(calls[0].starts_with("run --bg --json --no-parent --keep"));
    assert!(!calls.iter().any(|call| call.starts_with("send ")));
}
