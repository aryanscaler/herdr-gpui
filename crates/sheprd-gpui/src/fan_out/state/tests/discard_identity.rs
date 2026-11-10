use super::*;
use std::path::Path;

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .arg("-c")
        .arg(format!(
            "core.hooksPath={}",
            dir.join("unused-hooks").display()
        ))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn discarding_a_saved_lane_refuses_a_different_branch_or_repository() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    let lane = dir.path().join("lane");
    let other = dir.path().join("other");
    for repo in [&main, &other] {
        std::fs::create_dir(repo).unwrap();
        git(repo, &["init", "-b", "main"]);
        git(repo, &["commit", "--allow-empty", "-m", "initial"]);
    }
    git(
        &main,
        &["worktree", "add", "-b", "feature", lane.to_str().unwrap()],
    );
    git(&lane, &["switch", "-c", "other-work"]);
    std::fs::write(lane.join("uncommitted"), "keep me").unwrap();
    let worktree = work::Worktree {
        path: lane.to_str().unwrap().into(),
        opened: work::Opened::Herdr2,
    };
    let key = main.join(".git");
    let host = origin().host;
    let cancelled = AtomicBool::new(false);
    assert!(matches!(
        job::remove(
            &host,
            &worktree,
            key.to_str().unwrap(),
            "feature",
            &cancelled
        ),
        Err(Error::Checkout(_))
    ));
    git(&lane, &["switch", "feature"]);
    assert!(matches!(
        job::remove(
            &host,
            &worktree,
            other.join(".git").to_str().unwrap(),
            "feature",
            &cancelled
        ),
        Err(Error::Checkout(_))
    ));
    assert_eq!(
        std::fs::read_to_string(lane.join("uncommitted")).unwrap(),
        "keep me"
    );
}
