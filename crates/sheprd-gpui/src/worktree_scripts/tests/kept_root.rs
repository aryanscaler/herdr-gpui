//! A kept checkout's main checkout, which its scripts get as
//! `HERDR_ROOT_PATH`, read from Git in a throwaway repository.

use super::super::kept_root;
use std::{path::Path, process::Command, sync::atomic::AtomicBool};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn a_linked_checkout_names_its_main_checkout() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("app");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "a"]);
    let lane = temp.path().join("lane");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "lane",
            lane.to_str().unwrap(),
        ],
    );
    let root = kept_root(lane.to_str().unwrap(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        std::fs::canonicalize(root).unwrap(),
        std::fs::canonicalize(&main).unwrap()
    );
    assert_eq!(
        kept_root("/nonexistent/checkout", &AtomicBool::new(false)),
        None
    );
}
