//! Keep removes an owned lane's checkout only once its agent is ended, and
//! never while another program runs there. A stand-in `herdr-core` on an
//! isolated `HOME` answers; the lane is a real linked checkout with its
//! owner receipt, so only the core's inventory decides.

use super::*;
use std::{os::unix::fs::PermissionsExt, path::Path};

/// This computer with `HOME` at `home`, whose `herdr-core` logs each call
/// and answers `ls` with `terminals`.
fn host(home: &Path, terminals: &str) -> Host {
    let bin = home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = home.join("calls.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\nls) cat '{}' ;;\nkill) echo '{{\"terminals\":[]}}' > '{}' ;;\nesac\n",
        log.display(),
        home.join("inventory").display(),
        home.join("inventory").display(),
    );
    std::fs::write(home.join("inventory"), terminals).unwrap();
    let core = bin.join("herdr-core");
    std::fs::write(&core, script).unwrap();
    std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut host = origin().host;
    host.env = vec![("HOME".into(), home.display().to_string())];
    host
}

fn calls(home: &Path) -> Vec<String> {
    std::fs::read_to_string(home.join("calls.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

const OWNER: &str = "lane-a";

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A linked checkout on branch `lane` carrying `OWNER`'s creation receipt.
fn lane(dir: &Path) -> work::Worktree {
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let path = dir.join("lane").canonicalize().unwrap_or(dir.join("lane"));
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "lane",
            path.to_str().unwrap(),
        ],
    );
    let gitdir = std::process::Command::new("git")
        .arg("-C")
        .arg(&path)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .unwrap();
    let gitdir = String::from_utf8(gitdir.stdout).unwrap();
    std::fs::write(
        Path::new(gitdir.trim()).join("sheprd-fanout-owner"),
        format!("{OWNER}\n"),
    )
    .unwrap();
    work::Worktree {
        path: path.display().to_string(),
        opened: work::Opened::Herdr2,
    }
}

fn remove(host: &Host, tree: &work::Worktree) -> Result<()> {
    job::remove(
        host,
        tree,
        "/tmp/repo/.git",
        "lane",
        Some(OWNER),
        Some((AgentKind::Claude, Some("t9"))),
        &AtomicBool::new(false),
    )
}

#[test]
fn a_shell_in_the_lane_keeps_its_checkout_and_nothing_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let tree = lane(dir.path());
    let home = dir.path().join("home");
    let shell = format!(
        r#"{{"terminals":[{{"address":"t3","state":"live","cwd":"{}","argv":["sh"]}}]}}"#,
        tree.path
    );
    let host = host(&home, &shell);
    let removed = remove(&host, &tree);
    assert!(matches!(removed, Err(Error::AgentUncertain)), "{removed:?}");
    assert!(calls(&home).iter().all(|call| call.starts_with("ls ")));
    assert!(Path::new(&tree.path).exists());
}

#[test]
fn a_running_lane_agent_is_ended_before_its_checkout_goes() {
    let dir = tempfile::tempdir().unwrap();
    let tree = lane(dir.path());
    let home = dir.path().join("home");
    let agent = format!(
        r#"{{"terminals":[{{"address":"t9","state":"live","cwd":"{}","argv":["claude"],"labels":{{"sheprd.fanout.lane":"lane-a"}},"foreground":{{"name":"claude","argv":["claude"],"pid":9}}}}]}}"#,
        tree.path
    );
    let host = host(&home, &agent);
    remove(&host, &tree).unwrap();
    let calls = calls(&home);
    assert!(
        calls.iter().any(|call| call == "kill --json -- t9"),
        "{calls:?}"
    );
    assert!(!Path::new(&tree.path).exists());
}
