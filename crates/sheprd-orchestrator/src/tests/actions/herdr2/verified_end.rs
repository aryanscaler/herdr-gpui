//! Removing a Herdr 2 run's checkout only once herdr-core's inventory
//! proves nothing lives there: not the run's own terminal after its kill,
//! not another program, and not a shell a lost start reply left behind.
use super::*;

/// A core whose `ls` lists `listed` and whose kill may leave `t9` running.
fn core_listing(dir: &Path, listed: &str) {
    let script = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/core-calls"
case "$1" in
run) echo '{"address":"t9"}' ;;
wait) echo '{"ended":true,"exit_code":null,"reason":"status","status":"idle","turn":0}' ;;
ls) if [ -f "$HOME/listing" ]; then cat "$HOME/listing"; else echo '{"terminals":[]}'; fi ;;
*) echo '{}' ;;
esac
"#;
    test_executable::write(dir.join("home/.local/bin/herdr-core"), script, 0o755).unwrap();
    if !listed.is_empty() {
        std::fs::write(dir.join("home/listing"), listed).unwrap();
    }
}

fn live(address: &str, cwd: &Path) -> String {
    format!(
        r#"{{"address":"{address}","cwd":"{}","state":"live"}}"#,
        cwd.display()
    )
}

/// The run's own agent in `t9`, as herdr-core lists it: started with the
/// dispatch's owner label, command, and checkout, so Remove recognizes it.
fn owned_agent(dir: &Path) -> String {
    let calls = core_calls(dir);
    let run = calls.iter().find(|call| call.starts_with("run ")).unwrap();
    let words: Vec<&str> = run.split(' ').collect();
    let label = words[words.iter().position(|word| *word == "--label").unwrap() + 1];
    let (key, value) = label.split_once('=').unwrap();
    let argv = &words[words.iter().position(|word| *word == "--").unwrap() + 1..];
    let cwd = checkout(dir);
    serde_json::json!({
        "address": "t9",
        "cwd": cwd,
        "argv": argv,
        "labels": {key: value},
        "state": "live",
        "foreground": {"argv": argv, "name": argv[0], "cwd": cwd, "pid": 9},
    })
    .to_string()
}

fn dispatched(dir: &Path) -> (Site, String) {
    let site = site(dir, "");
    core_listing(dir, "");
    perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &AtomicBool::new(false),
    )
    .unwrap();
    let id = Store::open(&site.database).unwrap().runs().unwrap()[0]
        .id
        .clone();
    (site, id)
}

fn remove_is_refused(dir: &Path, site: &Site, id: String) {
    let error = perform(site, &Action::Remove { run: id }, &AtomicBool::new(false)).unwrap_err();
    assert!(
        matches!(
            error,
            Error::OwnedAgent(sheprd_remote::work::owned_agent::Error::Uncertain)
        ),
        "{error:?}"
    );
    assert!(checkout(dir).join(".git").exists());
    assert_eq!(
        Store::open(&site.database).unwrap().runs().unwrap().len(),
        1
    );
}

#[test]
fn a_run_whose_terminal_survives_its_kill_keeps_its_checkout_and_record() {
    let dir = tempfile::tempdir().unwrap();
    let (site, id) = dispatched(dir.path());
    let listing = format!(r#"{{"terminals":[{}]}}"#, owned_agent(dir.path()));
    std::fs::write(dir.path().join("home/listing"), listing).unwrap();
    remove_is_refused(dir.path(), &site, id);
    assert!(
        core_calls(dir.path())
            .iter()
            .any(|call| call == "keep --off --json -- t9")
    );
}

#[test]
fn another_live_program_in_the_checkout_keeps_it_after_the_run_ends() {
    let dir = tempfile::tempdir().unwrap();
    let (site, id) = dispatched(dir.path());
    let inside = checkout(dir.path()).join("src");
    let listing = format!(r#"{{"terminals":[{}]}}"#, live("t4", &inside));
    std::fs::write(dir.path().join("home/listing"), listing).unwrap();
    remove_is_refused(dir.path(), &site, id);
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.contains("t4"))
    );
}

#[test]
fn a_shell_left_by_a_lost_start_reply_keeps_the_rolled_back_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    // The shell starts, but its reply never arrives: no address to record.
    let script = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/core-calls"
case "$1" in
run) exit 75 ;;
ls) cat "$HOME/listing" ;;
*) echo '{}' ;;
esac
"#;
    test_executable::write(dir.path().join("home/.local/bin/herdr-core"), script, 0o755).unwrap();
    let listing = format!(r#"{{"terminals":[{}]}}"#, live("t9", &checkout(dir.path())));
    std::fs::write(dir.path().join("home/listing"), listing).unwrap();
    assert!(
        perform(
            &site,
            &Action::Dispatch(Box::new(herdr2_request())),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(checkout(dir.path()).join(".git").exists());
    let run = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0);
    assert_eq!(run.state, RunState::Failed);
    assert!(run.workspace.is_some());
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.starts_with("kill "))
    );
}
