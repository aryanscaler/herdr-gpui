use super::*;
use sheprd_client::{
    ConnectTarget,
    herdr::{
        AgentKind,
        sessions::{Binding, Identity, reference},
        snapshot::SessionKind,
    },
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn git(dir: &Path, args: &[&str]) {
    let output = sheprd_client::new_command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn script(home: &Path, program: &str, body: &str) {
    let path = home.join(".local/bin").join(program);
    write(&path, body);
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn binding(terminal: &str) -> Vec<u8> {
    serde_json::to_vec(&Binding {
        version: 1,
        sequence: 1,
        identity: Identity {
            core: "isolated-core".into(),
            terminal: terminal.into(),
            pid: 42,
            started: "opaque-birth".into(),
        },
        agent: "codex".into(),
        session: Some(reference(AgentKind::Codex, SessionKind::Id, "session-1".into()).unwrap()),
        environment: BTreeMap::new(),
    })
    .unwrap()
}

struct Fixture {
    _temp: tempfile::TempDir,
    source: Source,
    destination: Candidate,
    review: Review,
    source_home: PathBuf,
    destination_home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source_home = temp.path().join("source");
        let destination_home = temp.path().join("destination");
        let source_repo = source_home.join("repo");
        let destination_repo = destination_home.join("repo");
        fs::create_dir_all(&source_repo).unwrap();
        fs::create_dir_all(&destination_home).unwrap();
        git(&source_repo, &["init", "-q", "-b", "main"]);
        write(&source_repo.join("README"), "base\n");
        git(&source_repo, &["add", "README"]);
        git(&source_repo, &["commit", "-q", "-m", "base"]);
        git(
            temp.path(),
            &[
                "clone",
                "-q",
                source_repo.to_str().unwrap(),
                destination_repo.to_str().unwrap(),
            ],
        );
        git(&source_repo, &["switch", "-q", "-c", "feature"]);
        for home in [&source_home, &destination_home] {
            script(
                home,
                "herdr-core",
                r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$HOME/calls"
case "$1" in
  --version) echo test-core;;
  get) [ -f "$HOME/get-$3" ] || exit 3; cat "$HOME/get-$3";;
  run) echo '{"address":"t2"}';;
  send)
    [ ! -f "$HOME/fail-launch" ] || exit 1
    cp "$HOME/agent" "$HOME/get-t2"
    cp "$HOME/selected" "$HOME/binding-t2"
    ;;
  kill) :;;
  *) exit 2;;
esac
"#,
            );
            script(
                home,
                "sheprd-acp",
                "#!/bin/sh\n[ -f \"$HOME/binding-$2\" ] || exit 3\ncat \"$HOME/binding-$2\"\n",
            );
            script(home, "codex", "#!/bin/sh\nexit 0\n");
        }
        let terminal = serde_json::json!({"pid":1,"cwd":source_repo,"foreground":{"pid":42,"argv":["codex"],"cwd":source_repo},"status":{"app":"codex","state":"idle"}});
        write(
            &source_home.join("get-t1"),
            serde_json::to_vec(&terminal).unwrap(),
        );
        write(&source_home.join("binding-t1"), binding("t1"));
        write(
            &destination_home.join("agent"),
            serde_json::to_vec(&terminal).unwrap(),
        );
        write(&destination_home.join("selected"), binding("t2"));
        write(
            &source_home.join(".codex/sessions/2026/11/11/rollout-date-session-1.jsonl"),
            "{\"session\":\"session-1\"}\n",
        );
        let host = |home: &Path| {
            let mut host = Host::new(&ConnectTarget::Local).unwrap();
            host.env = vec![
                ("HOME".into(), home.to_string_lossy().into_owned()),
                (
                    "HC_SOCKET".into(),
                    home.join("isolated.sock").to_string_lossy().into_owned(),
                ),
                ("CODEX_HOME".into(), String::new()),
                ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
                ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ];
            host
        };
        let source = Source {
            place: Place {
                endpoint_id: "source".into(),
                label: "Source".into(),
                host: host(&source_home),
            },
            workspace_id: "source-space".into(),
            keeping: Keeping::App {
                checkout: source_repo.to_string_lossy().into_owned(),
            },
            custom_label: None,
            repo_key: source_repo.join(".git").to_string_lossy().into_owned(),
            repo_label: "repo".into(),
            branch: Some("feature".into()),
            tab_labels: HashMap::new(),
            core_tabs: vec![Panes::single(
                sheprd_backend::TerminalId::new("t1").unwrap(),
            )],
            core_preferred: true,
        };
        let destination = Candidate {
            place: Place {
                endpoint_id: "destination".into(),
                label: "Destination".into(),
                host: host(&destination_home),
            },
            destination: Destination::Open {
                repository: Repository {
                    key: destination_repo.join(".git").to_string_lossy().into_owned(),
                    label: "repo".into(),
                    workspace_id: String::new(),
                },
                reason: MatchReason::Name,
            },
            origin: None,
            lands_in: Keeper::App,
        };
        let review = review(&source, &destination, &AtomicBool::new(false)).unwrap();
        assert!(matches!(
            review.panes().next().unwrap().action,
            Action::Resume(AgentKind::Codex)
        ));
        Self {
            _temp: temp,
            source,
            destination,
            review,
            source_home,
            destination_home,
        }
    }

    fn source_stopped(&self) -> bool {
        fs::read_to_string(self.source_home.join("calls"))
            .unwrap_or_default()
            .lines()
            .any(|line| line == "kill --json t1")
    }

    fn destination_stopped(&self) -> bool {
        fs::read_to_string(self.destination_home.join("calls"))
            .unwrap_or_default()
            .lines()
            .any(|line| line == "kill --json t2")
    }
}

#[test]
fn copy_launch_and_persistence_failures_leave_source_running() {
    for phase in ["copy", "launch", "persist"] {
        let fixture = Fixture::new();
        match phase {
            "copy" => fs::remove_dir_all(fixture.source_home.join(".codex")).unwrap(),
            "launch" => write(&fixture.destination_home.join("fail-launch"), ""),
            _ => {}
        }
        let result = run(
            &fixture.source,
            &fixture.destination,
            &fixture.review,
            |_| {},
            |_| Err(Error::WorkerStopped),
            &AtomicBool::new(false),
        );
        assert!(result.is_err(), "{phase}");
        assert!(
            !fixture.source_stopped(),
            "{phase} must leave source running"
        );
        assert!(
            fixture.destination_stopped(),
            "{phase} must clean up owned destination terminal"
        );
    }
}

#[test]
fn acknowledged_arrival_is_persisted_before_retirement_and_is_not_cancelled_after_commit() {
    let fixture = Fixture::new();
    let cancelled = AtomicBool::new(false);
    let outcome = run(
        &fixture.source,
        &fixture.destination,
        &fixture.review,
        |_| {},
        |outcome| {
            assert!(!fixture.source_stopped());
            assert!(!outcome.source_retired);
            let receipt = crate::teleport::arrival::Receipt::new("destination", outcome);
            crate::teleport::arrival::save(&fixture.destination_home.join("receipts"), &receipt)?;
            cancelled.store(true, Ordering::Release);
            Ok(())
        },
        &cancelled,
    )
    .unwrap();
    assert!(outcome.source_retired);
    assert!(fixture.source_stopped());
    assert!(!fixture.destination_stopped());
    assert_eq!(
        crate::teleport::arrival::load(&fixture.destination_home.join("receipts"))
            .unwrap()
            .len(),
        1
    );
}
