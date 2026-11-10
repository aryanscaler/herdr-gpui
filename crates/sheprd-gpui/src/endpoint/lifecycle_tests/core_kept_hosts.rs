//! A workspace the app keeps on a saved host whose herdr-core turns out to
//! be missing: it has no Herdr to fall back to, so it goes once empty, and
//! the host keeps new workspaces in its Herdr from then on.

use super::*;
use crate::{
    browser::{Scope, Store},
    core_terminals::{Error, Host, Hub, Start},
};

fn ssh() -> ConnectTarget {
    ConnectTarget::Ssh {
        target: "fixture".into(),
        session: "default".into(),
    }
}

fn scope() -> Scope {
    Scope::endpoint("ssh:fixture")
}

fn missing(_: Start) -> std::result::Result<sheprd_core_client::CoreBackend, Error> {
    Err(Error::Bridge(
        sheprd_client::core_bridge::Error::Missing.into(),
    ))
}

#[gpui::test]
fn a_kept_workspace_on_a_host_without_a_core_goes_rather_than_retrying(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let (mut remote, _server) = connected_endpoint("ssh:fixture");
    remote.connection.target = ssh();
    view.update_in(cx, |view, window, cx| {
        view.endpoints.push(remote);
        view.selected_endpoint = 1;
        view.reset_selected(cx);
        view.config.herdr_2_terminals = true;
        let host = Host::Remote(scope());
        view.core_terminals
            .set_remote_hub_for_test(scope(), ssh(), Hub::with_join(missing));
        assert!(view.new_workspaces_use_core(), "not known to lack one yet");
        view.new_kept_workspace(Some("api"), None, cx);
        let kept = view.kept_in_front().map(str::to_owned).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while view.core_terminals.waits_on_for_test(&host) || !view.core_terminals.lacks_core(&host)
        {
            assert!(Instant::now() < deadline, "the attempt never finished");
            view.poll_core_terminals(window, cx);
            std::thread::yield_now();
        }
        let store = cx.global::<Store>();
        assert!(store.kept_workspace(&scope(), &kept).is_none(), "it went");
        assert!(view.kept_in_front().is_none());
        assert!(
            !view.core_terminals.waits_on_for_test(&host),
            "nothing retries"
        );
        assert!(
            !view.new_workspaces_use_core(),
            "the host's Herdr keeps them now"
        );
        assert!(!view.new_tabs_use_core());
    });
}

#[gpui::test]
fn a_hosts_first_shell_is_tried_again_after_a_passing_failure(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let (mut remote, _server) = connected_endpoint("ssh:fixture");
    remote.connection.target = ssh();
    view.update_in(cx, |view, window, cx| {
        view.endpoints.push(remote);
        view.selected_endpoint = 1;
        view.reset_selected(cx);
        // The host has no Herdr 1, and its core cannot be reached for now.
        view.live.ssh_failure = Some(SshFailure::HerdrMissing);
        let host = Host::Remote(scope());
        view.core_terminals.set_remote_hub_for_test(
            scope(),
            ssh(),
            Hub::with_join(|_| Err(Error::NotRemote)),
        );
        view.refresh_core_session(cx);
        let kept = |cx: &gpui::App| {
            cx.global::<Store>()
                .kept_workspace(&scope(), crate::core_terminals::WORKSPACE)
                .is_some()
        };
        assert!(kept(cx));
        assert!(view.core_terminals.waits_on_for_test(&host), "its shell");
        let deadline = Instant::now() + Duration::from_secs(5);
        while view.core_terminals.waits_on_for_test(&host) {
            assert!(Instant::now() < deadline, "the attempt never finished");
            view.poll_core_terminals(window, cx);
            std::thread::yield_now();
        }
        view.refresh_core_session(cx);
        // Gone while empty, and not kept again while the core cools down.
        assert!(!kept(cx), "the empty workspace went");
        assert!(
            !view.core_terminals.lacks_core(&host),
            "the core is not missing"
        );
        assert!(
            !view.core_terminals.waits_on_for_test(&host),
            "no dial too soon"
        );
        // Once it may be dialled again, the workspace and its shell come back.
        view.core_terminals
            .hub_of_mut_for_test(&host)
            .unwrap()
            .forget();
        view.refresh_core_session(cx);
        assert!(kept(cx));
        assert!(view.core_terminals.waits_on_for_test(&host), "tried again");
    });
}
