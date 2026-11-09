//! `serve.sh` itself, run by this computer's `/bin/sh` against a stand-in
//! px0, in a private home folder.
#![cfg(unix)]
use super::*;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

/// A stand-in px0: it keeps its arguments and state folder, leaves a child
/// of its own running, as px0 runs one per checkout, and then publishes its
/// address in `workbench.lock` as px0 does.
const FAKE_PX0: &str = r#"#!/bin/sh
printf '%s\n' "$@" > "$HOME/args"
printf '%s' "$XDG_STATE_HOME" > "$HOME/state"
sleep 600 &
echo $! > "$HOME/grandchild"
mkdir -p "$XDG_STATE_HOME/px0"
printf '{\n  "pid": %s,\n  "url": "http://127.0.0.1:41234"\n}\n' $$ > "$XDG_STATE_HOME/px0/workbench.lock"
echo $$ > "$HOME/pid"
wait
"#;

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hp0-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Puts the stand-in where the script looks when `ssh`'s PATH lacks it.
fn install(home: &Path) {
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let px0 = bin.join("px0");
    fs::write(&px0, FAKE_PX0).unwrap();
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
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.spawn().unwrap()
}

fn report(child: &mut Child) -> String {
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    line
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
fn the_script_reports_px0s_port_and_stops_its_whole_tree() {
    let home = home("serve");
    install(&home);
    let mut child = run(&home);
    assert_eq!(report(&mut child), "port 41234\n");
    let args = read(home.join("args"));
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        [
            "-no-open",
            "-no-update",
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
    // The end of the input, as the app goes away, stops px0 and its child.
    drop(child.stdin.take());
    assert!(wait_for(|| !alive(&px0) && !alive(&grandchild)));
    child.wait().unwrap();
    let _ = fs::remove_dir_all(home);
}

#[test]
fn a_running_workbench_is_reused_and_left_running() {
    let home = home("reuse");
    install(&home);
    let mut owner = run(&home);
    assert_eq!(report(&mut owner), "port 41234\n");
    let px0 = read(home.join("pid"));
    let mut second = run(&home);
    assert_eq!(report(&mut second), "port 41234\n");
    drop(second.stdin.take());
    assert!(second.wait().unwrap().success());
    assert!(alive(&px0), "only its starter stops it");
    drop(owner.stdin.take());
    assert!(wait_for(|| !alive(&px0)));
    owner.wait().unwrap();
    let _ = fs::remove_dir_all(home);
}

#[test]
fn a_host_without_px0_says_so() {
    let home = home("missing");
    let mut child = run(&home);
    let _ = child.stdin.take().map(|mut input| input.write_all(b""));
    let status = child.wait().unwrap();
    assert!(matches!(Error::exited(status), Error::Missing), "{status}");
    let _ = fs::remove_dir_all(home);
}
