use super::*;
use crate::remote_browser::install::SHIM;
use std::{
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
};

/// A host whose `herdr-core` is in `~/.local/bin` and records its arguments,
/// and the shim run in one of its terminals, outside any Herdr pane.
fn run(args: &[&str]) -> (tempfile::TempDir, Output) {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let write = |path: std::path::PathBuf, text: &str| {
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    write(home.path().join("herdr-gpui"), SHIM);
    write(
        bin.join("herdr-core"),
        "#!/bin/sh\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\"; done >\"$HOME/core.args\"\n",
    );
    let output = Command::new("/bin/sh")
        .arg(home.path().join("herdr-gpui"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("HC_TERMINAL_ID", "t3")
        .output()
        .unwrap();
    (home, output)
}

fn labels(home: &tempfile::TempDir) -> (Vec<String>, Vec<(String, String)>) {
    let text = std::fs::read_to_string(home.path().join("core.args")).unwrap();
    let args: Vec<String> = text.lines().map(str::to_owned).collect();
    let labels = args[2..]
        .iter()
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect();
    (args, labels)
}

#[test]
fn in_a_herdr_2_terminal_a_request_is_labels_on_that_terminal() {
    let address = format!("https://example.com/{}", "a".repeat(100));
    let (home, output) = run(&["browser", "open", "--no-focus", &address]);
    assert!(output.status.success(), "{output:?}");
    let (args, labels) = labels(&home);
    assert_eq!(&args[..2], ["label", "t3"]);
    let all = Request::all(&labels);
    assert_eq!(all.len(), 1, "{labels:?}");
    assert_eq!(all[0].pane, None);
    assert_eq!(
        all[0].action,
        Action::Open {
            address,
            focus: false
        }
    );
    // Every label is one of the request's, so the window can remove them all.
    assert!(labels.iter().all(|(name, _)| is_request_token(name)));
}

#[test]
fn a_reload_from_a_herdr_2_terminal_uses_its_own_labels() {
    let (home, output) = run(&["browser", "reload"]);
    assert!(output.status.success(), "{output:?}");
    let (args, labels) = labels(&home);
    assert_eq!(&args[..2], ["label", "t3"]);
    assert_eq!(Request::all(&labels)[0].action, Action::Reload);
}

#[test]
fn feedback_shim_round_trips_with_a_core_peer() {
    let output = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/tests/test_remote_browser_feedback.py"
        ))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
