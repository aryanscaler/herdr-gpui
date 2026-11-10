//! An update must not leave remote control off: a daemon of another version
//! restarts in place, and a reload waits for launchd to let the old job go.

use super::*;
use std::process::ExitStatus;

fn failed() -> Error {
    #[cfg(unix)]
    let status = <ExitStatus as std::os::unix::process::ExitStatusExt>::from_raw(5 << 8);
    #[cfg(windows)]
    let status = <ExitStatus as std::os::windows::process::ExitStatusExt>::from_raw(5);
    Error::Service {
        program: "launchctl",
        status,
        stderr: "Bootstrap failed: 5: Input/output error".into(),
    }
}

fn launchd(dir: &Path, release: &str) -> Host {
    Host {
        manager: Manager::LaunchAgent { uid: 501 },
        file: dir.join("LaunchAgents/so.pen.herdr-gpui.remote.plist"),
        release: Some(release.into()),
        ..host(dir)
    }
}

fn on() -> RemoteConfig {
    RemoteConfig {
        enabled: true,
        ..RemoteConfig::default()
    }
}

#[test]
fn a_daemon_of_another_version_restarts_without_leaving_launchd() {
    let dir = tempfile::tempdir().unwrap();
    let host = launchd(dir.path(), "20261108.1");
    reconcile(&on(), &host, |_| unreachable(), |_| Ok(())).unwrap();
    let commands = RefCell::new(Vec::new());
    let outcome = reconcile(
        &on(),
        &host,
        |_| Ok(status("20261106.1")),
        |step| {
            commands.borrow_mut().push(command(step));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(outcome, Outcome::Started);
    let commands = commands.into_inner();
    assert!(
        !commands.iter().any(|command| command.contains("bootout")),
        "{commands:?}"
    );
    assert_eq!(
        commands.last().map(String::as_str),
        Some("launchctl kickstart -k gui/501/so.pen.herdr-gpui.remote")
    );
}

#[test]
fn a_restart_of_an_unchanged_definition_restarts_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let host = launchd(dir.path(), "20261108.1");
    reconcile(&on(), &host, |_| unreachable(), |_| Ok(())).unwrap();
    let commands = RefCell::new(Vec::new());
    restart(&on(), &host, |step| {
        commands.borrow_mut().push(command(step));
        Ok(())
    })
    .unwrap();
    let commands = commands.into_inner();
    assert!(
        !commands.iter().any(|command| command.contains("bootout")),
        "{commands:?}"
    );
    assert_eq!(
        commands.last().map(String::as_str),
        Some("launchctl kickstart -k gui/501/so.pen.herdr-gpui.remote")
    );
}

#[test]
fn a_reload_retries_the_bootstrap_until_launchd_lets_the_old_job_go() {
    let load = Manager::LaunchAgent { uid: 501 }.load(Path::new("/f"));
    let bootstrap = load.last().unwrap();
    assert!(bootstrap.settles && bootstrap.required);
    assert!(!load[0].settles, "a bootout is tried once");
    let (tries, waits) = (RefCell::new(0), RefCell::new(0));
    settle(
        bootstrap,
        20,
        || {
            *tries.borrow_mut() += 1;
            if *tries.borrow() < 3 {
                Err(failed())
            } else {
                Ok(())
            }
        },
        || *waits.borrow_mut() += 1,
    )
    .unwrap();
    assert_eq!((tries.into_inner(), waits.into_inner()), (3, 2));
}

#[test]
fn a_bootstrap_that_never_settles_fails_after_its_bounded_tries() {
    let load = Manager::LaunchAgent { uid: 501 }.load(Path::new("/f"));
    let tries = RefCell::new(0);
    let result = settle(
        load.last().unwrap(),
        4,
        || {
            *tries.borrow_mut() += 1;
            Err(failed())
        },
        || {},
    );
    assert!(matches!(result, Err(Error::Service { .. })), "{result:?}");
    assert_eq!(tries.into_inner(), 4);
}

#[test]
fn other_steps_are_tried_once() {
    let unload = Manager::LaunchAgent { uid: 501 }.unload();
    let tries = RefCell::new(0);
    let result = settle(
        &unload[0],
        20,
        || {
            *tries.borrow_mut() += 1;
            Err(failed())
        },
        || panic!("an unsettled step never waits"),
    );
    assert!(result.is_err());
    assert_eq!(tries.into_inner(), 1);
}
