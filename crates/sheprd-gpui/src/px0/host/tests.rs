#![allow(clippy::unwrap_used)]
use super::*;
use crate::browser::WebUrl;

mod script;

#[test]
fn only_the_scripts_own_report_names_a_port() {
    assert_eq!(
        worker::parse("port 35165\n").map(|port| port.get()),
        Some(35165)
    );
    for line in [
        "port 0\n",
        "port 70000\n",
        "port\n",
        "35165\n",
        "",
        "listening on 1\n",
    ] {
        assert_eq!(worker::parse(line), None, "{line:?}");
    }
}

#[cfg(unix)]
#[test]
fn the_scripts_exit_codes_name_its_failures() {
    use std::os::unix::process::ExitStatusExt;
    let named = |code: i32| Error::exited(std::process::ExitStatus::from_raw(code << 8));
    assert!(matches!(named(2), Error::Setup));
    assert!(matches!(named(3), Error::Missing));
    assert!(matches!(named(4), Error::Address));
    assert!(matches!(named(5), Error::Exited));
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
