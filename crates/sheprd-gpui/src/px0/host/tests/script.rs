//! `serve.sh` itself, run by this computer's `/bin/sh` against a stand-in
//! px0, in a private home folder.
#![cfg(unix)]
use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

/// A stand-in px0: it keeps its arguments and state folder and leaves a
/// child of its own running, as px0 runs one per checkout. Hung up, it stops
/// that child and exits, as px0 does.
const FAKE_PX0: &str = r#"#!/bin/sh
printf '%s\n' "$@" > "$HOME/args"
printf '%s' "$XDG_STATE_HOME" > "$HOME/state"
sleep 600 &
child=$!
trap 'kill "$child"; exit 0' HUP
echo "$child" > "$HOME/grandchild"
echo $$ > "$HOME/pid"
wait
"#;

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hp0-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Puts `program` where the script looks when `ssh`'s PATH lacks px0.
fn install(home: &Path, program: &str) {
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let px0 = bin.join("px0");
    fs::write(&px0, program).unwrap();
    fs::set_permissions(&px0, fs::Permissions::from_mode(0o700)).unwrap();
}

fn run(home: &Path) -> Child {
    #[allow(clippy::disallowed_methods)]
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", worker::SCRIPT])
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().unwrap()
}

fn read(path: PathBuf) -> String {
    fs::read_to_string(path).unwrap().trim().to_owned()
}

fn alive(pid: &str) -> bool {
    #[allow(clippy::disallowed_methods)]
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..500 {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn the_script_starts_px0_and_hangs_it_up_when_its_input_ends() {
    let home = home("serve");
    install(&home, FAKE_PX0);
    let mut child = run(&home);
    assert!(wait_for(
        || home.join("pid").exists() && home.join("grandchild").exists()
    ));
    let args = read(home.join("args"));
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        [
            "-no-open",
            "-no-update",
            "-no-telemetry",
            "-no-color",
            "-host",
            "127.0.0.1",
            "-port",
            "0",
            "-base-path",
            "/"
        ]
    );
    assert_eq!(
        read(home.join("state")),
        home.join(".herdr-gpui/px0-state").to_string_lossy()
    );
    let (px0, grandchild) = (read(home.join("pid")), read(home.join("grandchild")));
    assert!(alive(&px0) && alive(&grandchild));
    // The end of the input, as the app goes away, hangs up px0, which stops
    // its child; the script then ends with px0.
    drop(child.stdin.take());
    assert!(wait_for(|| !alive(&px0) && !alive(&grandchild)));
    assert!(child.wait().unwrap().success());
    let _ = fs::remove_dir_all(home);
}

#[test]
fn the_script_ends_with_px0_and_its_status() {
    let home = home("exits");
    install(&home, "#!/bin/sh\nexit 3\n");
    let mut child = run(&home);
    // The input stays open (`Child::wait` would close it first): px0's own
    // end is what ends the script.
    let mut status = None;
    assert!(wait_for(|| {
        status = child.try_wait().unwrap();
        status.is_some()
    }));
    let status = status.unwrap();
    assert!(matches!(Error::exited(status), Error::Stopped(status) if status.code() == Some(3)));
    let _ = fs::remove_dir_all(home);
}

#[test]
fn a_host_without_px0_says_so() {
    let home = home("missing");
    let mut child = run(&home);
    drop(child.stdin.take());
    let status = child.wait().unwrap();
    assert!(matches!(Error::exited(status), Error::Missing), "{status}");
    let _ = fs::remove_dir_all(home);
}

#[test]
fn only_a_live_px0_of_the_users_own_counts_as_running() {
    let check = |pid: u32| {
        #[allow(clippy::disallowed_methods)]
        let output = Command::new("/bin/sh")
            .args(["-c", &worker::alive_script(pid)])
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout.starts_with(b"alive")
    };
    assert!(check(std::process::id()));
    #[allow(clippy::disallowed_methods)]
    let mut gone = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    assert!(!check(pid));
    // Another account's process, as launchd's is, is not this user's.
    assert!(!check(1));
}

#[test]
fn px0s_state_is_private_to_the_user() {
    let home = home("umask");
    install(&home, "#!/bin/sh\numask > \"$HOME/umask\"\n");
    let mut child = run(&home);
    assert!(wait_for(|| child.try_wait().unwrap().is_some()));
    assert_eq!(read(home.join("umask")), "0077");
    let _ = fs::remove_dir_all(home);
}
