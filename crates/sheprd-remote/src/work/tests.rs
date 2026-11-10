use super::*;
use sheprd_client::ConnectTarget;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .envs(GIT_ENV)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

const GIT_ENV: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "t"),
    ("GIT_AUTHOR_EMAIL", "t@t"),
    ("GIT_COMMITTER_NAME", "t"),
    ("GIT_COMMITTER_EMAIL", "t@t"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
];

struct Fixture {
    temp: tempfile::TempDir,
    repo: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(home.join(".local/bin")).unwrap();
        let repo = temp.path().join("My Repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        fs::write(repo.join("a.txt"), "a").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-q", "-m", "a"]);
        Self { temp, repo, home }
    }

    /// A host whose home is the fixture's, so checkouts and the stand-in
    /// `herdr` and `herdr-core` programs stay inside it.
    fn host(&self) -> Host {
        let mut host = Host::new(&ConnectTarget::Local).unwrap();
        host.env = GIT_ENV
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .chain([("HOME".to_owned(), self.home.display().to_string())])
            .collect();
        host
    }

    fn log(&self) -> PathBuf {
        self.temp.path().join("calls.log")
    }

    /// A stand-in program on the host's `PATH` that logs its arguments and
    /// runs `body`.
    fn program(&self, name: &str, body: &str) {
        let path = self.home.join(".local/bin").join(name);
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n{body}\n",
            shell_quote(&self.log().display().to_string())
        );
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn herdr2(&self) -> Repository {
        Repository::Herdr2 {
            checkout: self.repo.display().to_string(),
        }
    }
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn a_herdr_2_worktree_lands_where_herdr_1_files_it() {
    let fixture = Fixture::new();
    let new = NewWorktree {
        branch: "Fix/Login_Bug",
        base: "HEAD",
        label: None,
    };
    let created = create(&fixture.host(), &fixture.herdr2(), &new, &never()).unwrap();
    assert_eq!(created.opened, Opened::Herdr2);
    let expected = fixture.home.join(".herdr/worktrees/My Repo/fix-login-bug");
    assert_eq!(PathBuf::from(&created.path), expected);
    assert_eq!(
        git(&expected, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "Fix/Login_Bug"
    );
}

#[test]
fn an_existing_branch_is_checked_out_as_it_stands() {
    let fixture = Fixture::new();
    git(&fixture.repo, &["branch", "kept"]);
    fs::write(fixture.repo.join("b.txt"), "b").unwrap();
    git(&fixture.repo, &["add", "b.txt"]);
    git(&fixture.repo, &["commit", "-q", "-m", "b"]);
    let kept = git(&fixture.repo, &["rev-parse", "kept"]);
    let new = NewWorktree {
        branch: "kept",
        base: "HEAD",
        label: None,
    };
    let created = create(&fixture.host(), &fixture.herdr2(), &new, &never()).unwrap();
    assert_eq!(git(Path::new(&created.path), &["rev-parse", "HEAD"]), kept);
}

#[test]
fn a_refused_worktree_says_why() {
    let fixture = Fixture::new();
    let new = NewWorktree {
        branch: "x",
        base: "no-such-ref",
        label: None,
    };
    let error = create(&fixture.host(), &fixture.herdr2(), &new, &never()).unwrap_err();
    assert!(
        matches!(
            error,
            Error::Script {
                step: Step::CreateWorktree,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn listing_and_removing_keep_the_branch_and_spare_changes() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let new = NewWorktree {
        branch: "lane",
        base: "HEAD",
        label: None,
    };
    let created = create(&host, &fixture.herdr2(), &new, &never()).unwrap();
    let listed = list(&host, &created.path, &never()).unwrap();
    let branches: Vec<_> = listed.iter().map(|c| c.branch.as_deref()).collect();
    assert_eq!(branches, [Some("main"), Some("lane")]);
    assert_eq!(
        fs::canonicalize(&listed[1].path).unwrap(),
        fs::canonicalize(&created.path).unwrap()
    );

    fs::write(Path::new(&created.path).join("wip.txt"), "wip").unwrap();
    assert!(matches!(
        remove(&host, &created.path, false, &never()),
        Err(Error::Dirty)
    ));
    assert!(Path::new(&created.path).exists());
    remove(&host, &created.path, true, &never()).unwrap();
    assert!(!Path::new(&created.path).exists());
    assert_eq!(git(&fixture.repo, &["branch", "--list", "lane"]), "lane");
}

#[test]
fn a_herdr_2_agent_starts_in_a_shell_and_takes_its_prompt_once_ready() {
    let fixture = Fixture::new();
    fixture.program(
        "herdr-core",
        r#"case "$1" in
run) echo '{"address":"t9"}' ;;
wait) echo '{"ended":true,"exit_code":null,"reason":"status","status":"idle","turn":0}' ;;
send) echo '{}' ;;
esac"#,
    );
    let worktree = Worktree {
        path: "/work/lane one".into(),
        opened: Opened::Herdr2,
    };
    let agent = Agent {
        kind: AgentKind::Claude,
        name: "claude-1",
        model: None,
        prompt: Some("fix it"),
    };
    let started = start_agent(&fixture.host(), &worktree, &agent, &never()).unwrap();
    assert_eq!(
        started,
        Started::Herdr2 {
            terminal: "t9".into()
        }
    );
    assert_eq!(
        fixture.calls(),
        [
            "run --bg --json --no-parent --cwd /work/lane one",
            "send --json -- t9 'claude'",
            "wait --json --timeout 5 t9",
            "send --json -- t9 fix it",
        ]
    );
}

#[test]
fn a_herdr_2_agent_stuck_on_a_dialog_is_not_prompted() {
    let fixture = Fixture::new();
    fixture.program(
        "herdr-core",
        r#"case "$1" in
run) echo '{"address":"t2"}' ;;
wait) echo '{"ended":true,"exit_code":null,"reason":"status","status":"blocked","turn":0}' ;;
esac"#,
    );
    let worktree = Worktree {
        path: "/w".into(),
        opened: Opened::Herdr2,
    };
    let agent = Agent {
        kind: AgentKind::Codex,
        name: "codex-1",
        model: None,
        prompt: Some("go"),
    };
    let error = start_agent(&fixture.host(), &worktree, &agent, &never()).unwrap_err();
    // The agent runs, so its terminal is named for the caller to show.
    let Error::Unprompted { started, source } = error else {
        panic!("{error:?}");
    };
    assert_eq!(
        started,
        Started::Herdr2 {
            terminal: "t2".into()
        }
    );
    assert!(
        matches!(
            *source,
            Error::Agent(sheprd_core_client::Error::AgentNotReady { .. })
        ),
        "{source:?}"
    );
    assert!(!fixture.calls().iter().any(|call| call.ends_with(" go")));
}

#[test]
fn herdr_1_makes_its_own_workspace_and_pane() {
    let fixture = Fixture::new();
    fixture.program(
        "herdr",
        r#"case "$1" in
worktree) echo '{"result":{"workspace":{"workspace_id":"w7"},"tab":{"tab_id":"w7:1"},"root_pane":{"pane_id":"w7-1","terminal_id":null},"worktree":{"path":"/x/lane"}}}' ;;
agent) echo '{}' ;;
esac"#,
    );
    let repository = Repository::Herdr1 {
        workspace_id: "w1".into(),
    };
    let new = NewWorktree {
        branch: "lane",
        base: "abc123",
        label: Some("Lane"),
    };
    let agent = Agent {
        kind: AgentKind::Claude,
        name: "claude-2",
        model: None,
        prompt: Some("-hello"),
    };
    let (worktree, started) = start(&fixture.host(), &repository, &new, &agent, &never()).unwrap();
    assert_eq!(
        worktree,
        Worktree {
            path: "/x/lane".into(),
            opened: Opened::Herdr1 {
                workspace_id: "w7".into(),
                pane_id: "w7-1".into(),
            },
        }
    );
    assert_eq!(
        started,
        Started::Herdr1 {
            pane_id: "w7-1".into()
        }
    );
    let calls = fixture.calls();
    assert_eq!(
        calls[0],
        "worktree create --workspace w1 --branch lane --base abc123 --no-focus --label Lane"
    );
    assert_eq!(
        calls[1],
        "agent start claude-2 --kind claude --pane w7-1 --timeout 90000"
    );
    // Herdr 1's CLI takes no `--`, so a prompt never reads as an option.
    assert_eq!(calls[2], "agent prompt w7-1  -hello");
}

#[test]
fn porcelain_records_become_checkouts() {
    let output = "worktree /r\0HEAD 1\0branch refs/heads/main\0\0worktree /r/x\0HEAD 2\0detached\0prunable gone\0\0worktree /b\0bare\0\0";
    assert_eq!(
        parse_list(output),
        [
            Checkout {
                path: "/r".into(),
                branch: Some("main".into()),
                ..Checkout::default()
            },
            Checkout {
                path: "/r/x".into(),
                detached: true,
                prunable: true,
                ..Checkout::default()
            },
            Checkout {
                path: "/b".into(),
                bare: true,
                ..Checkout::default()
            },
        ]
    );
}

#[test]
fn a_shell_whose_agent_never_started_is_ended() {
    let fixture = Fixture::new();
    fixture.program(
        "herdr-core",
        r#"case "$1" in
run) echo '{"address":"t4"}' ;;
send) exit 1 ;;
kill) echo '{}' ;;
esac"#,
    );
    let worktree = Worktree {
        path: "/w".into(),
        opened: Opened::Herdr2,
    };
    let agent = Agent {
        kind: AgentKind::Claude,
        name: "claude-3",
        model: None,
        prompt: None,
    };
    let error = start_agent(&fixture.host(), &worktree, &agent, &never()).unwrap_err();
    assert!(
        matches!(
            error,
            Error::Script {
                step: Step::StartAgent,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(fixture.calls().last().unwrap(), "kill --json -- t4");
}

#[test]
fn a_checkout_is_removed_from_any_of_its_folders() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let new = NewWorktree {
        branch: "deep",
        base: "HEAD",
        label: None,
    };
    let created = create(&host, &fixture.herdr2(), &new, &never()).unwrap();
    let inner = Path::new(&created.path).join("src");
    fs::create_dir(&inner).unwrap();
    remove(&host, &inner.display().to_string(), false, &never()).unwrap();
    assert!(!Path::new(&created.path).exists());
}

mod discard;
mod model;
