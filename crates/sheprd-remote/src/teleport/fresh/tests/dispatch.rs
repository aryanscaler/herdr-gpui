//! Dispatch's worktree on each Herdr: Herdr 1's `herdr worktree create`
//! with the arguments it always had, and on Herdr 2 Git, with no `herdr`.

use super::*;
use crate::teleport::job::Repository as Open;
use sheprd_client::shell_quote;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

const GIT_ENV: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "t"),
    ("GIT_AUTHOR_EMAIL", "t@t"),
    ("GIT_COMMITTER_NAME", "t"),
    ("GIT_COMMITTER_EMAIL", "t@t"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
];

/// A home of its own, holding a stand-in `herdr` that logs its arguments
/// and answers as `herdr worktree create` does.
struct Home {
    _temp: tempfile::TempDir,
    path: PathBuf,
    log: PathBuf,
}

impl Home {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("home");
        let bin = path.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        let log = temp.path().join("calls.log");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\necho '{}'\n",
            shell_quote(&log.display().to_string()),
            r#"{"result":{"workspace":{"workspace_id":"w7"},"tab":{"tab_id":"w7:1"},"root_pane":{"pane_id":"w7-1","terminal_id":null},"worktree":{"path":"/x/lane"}}}"#,
        );
        fs::write(bin.join("herdr"), script).unwrap();
        fs::set_permissions(bin.join("herdr"), fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _temp: temp,
            path,
            log,
        }
    }

    fn host(&self) -> Host {
        let mut host = host();
        host.env = GIT_ENV
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .chain([("HOME".to_owned(), self.path.display().to_string())])
            .collect();
        host
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn prepared(host: Host) -> Prepared {
    Prepared {
        repository: Open {
            key: "/repo/.git".into(),
            label: "repo".into(),
            workspace_id: "w1".into(),
        },
        host,
        reference: None,
    }
}

#[test]
fn herdr_1_names_a_worktree_left_unnamed() {
    let home = Home::new();
    let naming = Naming::default();
    let created = create_worktree(
        &prepared(home.host()),
        "abc123",
        &naming,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(created, ("/x/lane".to_owned(), "w7".to_owned()));
    // No `--branch`: the daemon picks the name, as it always did.
    assert_eq!(
        home.calls(),
        ["worktree create --workspace w1 --base abc123 --no-focus"]
    );
}

#[test]
fn herdr_1_takes_the_branch_and_label_given() {
    let home = Home::new();
    let naming = Naming {
        branch: Some("lane".into()),
        label: Some("Lane".into()),
    };
    create_worktree(
        &prepared(home.host()),
        "abc123",
        &naming,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        home.calls(),
        ["worktree create --workspace w1 --branch lane --base abc123 --no-focus --label Lane"]
    );
}

fn kept_origin(fixture: &Fixture) -> Origin {
    Origin {
        place: Place {
            endpoint_id: "box".into(),
            label: "Box".into(),
            host: host(),
        },
        workspace_id: "kept-1".into(),
        repo_key: fixture.key.clone(),
        repo_label: "origin repo".into(),
    }
}

/// This computer with the destination clone open in a kept workspace.
fn kept_destination(fixture: &Fixture, home: &Home) -> HostRepositories {
    HostRepositories {
        lands_in: sheprd_session::Keeper::App,
        place: Place {
            endpoint_id: "local".into(),
            label: "This Mac".into(),
            host: home.host(),
        },
        repositories: Some(vec![Open {
            key: fixture.destination.clone(),
            label: "origin repo".into(),
            workspace_id: "kept-2".into(),
        }]),
        retired: Vec::new(),
    }
}

#[test]
fn herdr_2_ships_the_base_and_makes_the_worktree_with_git() {
    let fixture = fixture();
    let home = Home::new();
    let mut steps = Vec::new();
    let created = dispatch_kept_worktree(
        &kept_origin(&fixture),
        &kept_destination(&fixture, &home),
        "refs/heads/feature",
        "lane",
        |step| steps.push(step),
        &AtomicBool::new(false),
    )
    .unwrap();
    // Herdr 1's folder for it, under the destination's home.
    let expected = home.path.join(".herdr/worktrees/destination/lane");
    assert_eq!(PathBuf::from(&created.path), expected);
    // Its setup script reads the repository and main checkout it came from.
    assert!(
        created
            .root
            .as_deref()
            .is_some_and(|root| root.ends_with("/destination")),
        "{:?}",
        created.root
    );
    assert!(!created.repo_key.is_empty());
    assert_eq!(git(&expected, &["rev-parse", "HEAD"]), fixture.feature);
    assert_eq!(
        git(&expected, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "lane"
    );
    assert!(steps.contains(&Step::Transfer), "the base was shipped");
    let destination = Path::new(&fixture.destination);
    assert_eq!(
        git(destination, &["for-each-ref", "refs/herdr-teleport"]),
        "",
        "the temporary reference is dropped"
    );
    assert!(home.calls().is_empty(), "Herdr 1 is never asked");
}

#[test]
fn herdr_2_workspace_is_the_main_checkout() {
    let fixture = fixture();
    let home = Home::new();
    let path = dispatch_kept_workspace(
        &kept_origin(&fixture),
        &kept_destination(&fixture, &home),
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let main = Path::new(&fixture.destination).parent().unwrap();
    assert_eq!(
        fs::canonicalize(path).unwrap(),
        fs::canonicalize(main).unwrap()
    );
    assert!(home.calls().is_empty(), "Herdr 1 is never asked");
}
