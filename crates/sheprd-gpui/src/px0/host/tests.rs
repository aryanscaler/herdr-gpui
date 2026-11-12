#![allow(clippy::unwrap_used)]
use super::*;
use crate::browser::WebUrl;

mod script;

#[test]
fn the_hosts_lock_is_read_as_this_computers_is() {
    use super::super::server;
    let found =
        server::parse(b"{\n  \"pid\": 4242,\n  \"url\": \"http://127.0.0.1:41234\"\n}\n").unwrap();
    assert_eq!(found.pid, 4242);
    assert_eq!(
        found.url,
        WebUrl::try_from("http://127.0.0.1:41234").unwrap()
    );
    // No lock, a stale or foreign record, or an oversized one names nothing.
    for bytes in [
        &b""[..],
        b"not json",
        b"{\"pid\": 0, \"url\": \"http://127.0.0.1:41234\"}",
        b"{\"pid\": 7, \"url\": \"http://example.com:41234\"}",
    ] {
        assert!(server::parse(bytes).is_none(), "{bytes:?}");
    }
    let long = vec![b' '; usize::try_from(server::LOCK_LIMIT).unwrap() + 1];
    assert!(server::parse(&long).is_none());
}

#[cfg(unix)]
#[test]
fn the_scripts_exit_codes_name_its_failures() {
    use std::os::unix::process::ExitStatusExt;
    let named = |code: i32| Error::exited(std::process::ExitStatus::from_raw(code << 8));
    assert!(matches!(named(127), Error::Missing));
    assert!(matches!(named(255), Error::Ssh));
    assert!(matches!(named(1), Error::Stopped(status) if status.code() == Some(1)));
}

#[gpui::test]
fn each_host_keeps_its_own_workbench_until_reset(cx: &mut gpui::TestAppContext) {
    let url = WebUrl::try_from("http://127.0.0.1:41234/").unwrap();
    cx.update(|cx| {
        Hosts::fixture(
            cx,
            [
                ("dev@build".to_owned(), State::Ready(url.clone())),
                ("dev@other".to_owned(), State::Starting),
            ],
        );
        assert!(matches!(Hosts::state(cx, "dev@build"), State::Ready(found) if found == url));
        assert!(matches!(Hosts::state(cx, "dev@other"), State::Starting));
        assert!(matches!(Hosts::state(cx, "dev@unknown"), State::Idle));
        // A host that has a workbench, or a failure, is not started again.
        Hosts::want(cx, "dev@build");
        assert!(matches!(Hosts::state(cx, "dev@build"), State::Ready(_)));
        Hosts::reset(cx, "dev@build");
        assert!(matches!(Hosts::state(cx, "dev@build"), State::Idle));
        assert!(matches!(Hosts::state(cx, "dev@other"), State::Starting));
    });
}
