//! Discarding a worktree, as fan-out removes the lanes it does not keep.

use super::*;

#[test]
fn herdr_1_discards_through_its_workspace() {
    let fixture = Fixture::new();
    fixture.program("herdr", "echo '{}'");
    let worktree = Worktree {
        path: "/x/lane".into(),
        opened: Opened::Herdr1 {
            workspace_id: "w7".into(),
            pane_id: "w7-1".into(),
        },
    };
    discard(&fixture.host(), &worktree, &never()).unwrap();
    assert_eq!(fixture.calls(), ["worktree remove --workspace w7 --force"]);
}

#[test]
fn herdr_2_discards_changes_and_keeps_the_branch() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let new = NewWorktree {
        branch: "lane",
        base: "HEAD",
        label: None,
    };
    let created = create(&host, &fixture.herdr2(), &new, &never()).unwrap();
    fs::write(Path::new(&created.path).join("wip.txt"), "wip").unwrap();
    discard(&host, &created, &never()).unwrap();
    assert!(!Path::new(&created.path).exists());
    assert_eq!(git(&fixture.repo, &["branch", "--list", "lane"]), "lane");
}
