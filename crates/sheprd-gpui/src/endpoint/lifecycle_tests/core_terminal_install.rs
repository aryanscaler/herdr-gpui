//! A host's herdr-core seen from the window's other features: installing
//! one clears what the window remembered of a host without it, and the
//! agents working there count when dispatch ranks hosts.

use super::*;
use crate::{
    browser::Scope,
    core_terminals::{Error, Host, Hub, Start, State},
};
use sheprd_backend::{RunState, TerminalId, TerminalInfo};
use sheprd_client::protocol::AgentStatus;

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

fn agent(id: &str, agent: AgentStatus) -> TerminalInfo {
    TerminalInfo {
        agent,
        cwd: None,
        foreground: None,
        id: TerminalId::new(id).unwrap(),
        labels: Default::default(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: None,
    }
}

#[gpui::test]
fn installing_herdr_core_lets_the_host_take_herdr_2_tabs_again(cx: &mut gpui::TestAppContext) {
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
        view.core_terminals
            .hub_of_mut_for_test(&host)
            .unwrap()
            .connect(Start::IfMissing);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !view.core_terminals.lacks_core(&host) {
            assert!(Instant::now() < deadline, "the attempt never finished");
            view.poll_core_terminals(window, cx);
            std::thread::yield_now();
        }
        assert!(!view.new_tabs_use_core());
        view.core_install_ended(
            host.clone(),
            "fixture",
            Ok(sheprd_remote::core_install::Outcome::Installed),
            cx,
        );
        assert!(!view.core_terminals.lacks_core(&host));
        assert!(matches!(
            view.core_terminals.hub_of(&host).map(Hub::state),
            Some(State::Idle)
        ));
        assert!(view.new_tabs_use_core());
    });
}

#[gpui::test]
fn a_disconnected_hosts_herdr_core_agents_do_not_count_when_ranking_it(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let (mut remote, _server) = connected_endpoint("ssh:fixture");
    remote.connection.target = ssh();
    view.update(cx, |view, cx| {
        view.endpoints[0].connection.target = ConnectTarget::Local;
        view.endpoints.push(remote);
        let host = Host::Remote(scope());
        view.core_terminals
            .set_remote_hub_for_test(scope(), ssh(), Hub::with_join(missing));
        for (id, status) in [
            ("t1", AgentStatus::Working),
            ("t2", AgentStatus::Working),
            ("t3", AgentStatus::Idle),
        ] {
            view.core_terminals
                .record_for_test(&host, agent(id, status));
        }
        // Only a connected core keeps its list current: one that never
        // answered counts none of the agents it last reported, even with a
        // tab of it shown here.
        assert_eq!(view.core_terminals.working(&host), 0);
        view.core_terminals.show_on_host_for_test(
            crate::browser::TabId::test(1),
            host.clone(),
            TerminalId::new("t1").unwrap(),
        );
        assert_eq!(view.core_terminals.working(&host), 0);
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return;
        }
        let candidates = view.dispatch_candidates("repo", cx);
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.endpoint_id == "ssh:fixture")
            .unwrap();
        assert_eq!(candidate.working, 0);
    });
}

#[gpui::test]
fn a_second_install_on_a_device_waits_for_the_first(cx: &mut gpui::TestAppContext) {
    fn present(
        _: &ConnectTarget,
    ) -> std::result::Result<
        sheprd_remote::core_install::Outcome,
        crate::core_terminals::install::Failed,
    > {
        Ok(sheprd_remote::core_install::Outcome::Present)
    }
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let (mut remote, _server) = connected_endpoint("ssh:fixture");
    remote.connection.target = ssh();
    remote.label = "box".into();
    view.update(cx, |view, cx| {
        view.endpoints.push(remote);
        view.install_core_with("ssh:fixture", present, cx);
        view.install_core_with("ssh:fixture", present, cx);
        let (flash, _) = view.flash.as_ref().unwrap();
        assert_eq!(
            flash.text,
            crate::i18n::fl!("core-terminals-install-running", device = "box")
        );
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        let (flash, _) = view.flash.as_ref().unwrap();
        assert_eq!(
            flash.text,
            crate::i18n::fl!("core-terminals-install-present", device = "box")
        );
    });
    // Once it ended, the device takes another.
    view.update(cx, |view, cx| {
        view.install_core_with("ssh:fixture", present, cx);
        let (flash, _) = view.flash.as_ref().unwrap();
        assert_eq!(
            flash.text,
            crate::i18n::fl!("core-terminals-install-started", device = "box")
        );
    });
}

#[gpui::test]
fn an_install_clears_the_missing_core_in_every_window(cx: &mut gpui::TestAppContext) {
    fn installed(
        _: &ConnectTarget,
    ) -> std::result::Result<
        sheprd_remote::core_install::Outcome,
        crate::core_terminals::install::Failed,
    > {
        Ok(sheprd_remote::core_install::Outcome::Installed)
    }
    let host = Host::Remote(scope());
    let mut views = Vec::new();
    let mut servers = Vec::new();
    for _ in 0..2 {
        let (view, window) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
        let (mut remote, server) = connected_endpoint("ssh:fixture");
        remote.connection.target = ssh();
        servers.push(server);
        view.update_in(window, |view, window, cx| {
            view.endpoints.push(remote);
            view.core_terminals
                .set_remote_hub_for_test(scope(), ssh(), Hub::with_join(missing));
            view.core_terminals
                .hub_of_mut_for_test(&host)
                .unwrap()
                .connect(Start::IfMissing);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !view.core_terminals.lacks_core(&host) {
                assert!(Instant::now() < deadline, "the attempt never finished");
                view.poll_core_terminals(window, cx);
                std::thread::yield_now();
            }
        });
        views.push(view);
    }
    views[0].update(cx, |view, cx| {
        view.install_core_with("ssh:fixture", installed, cx);
    });
    cx.run_until_parked();
    for view in &views {
        view.update(cx, |view, _| {
            assert!(!view.core_terminals.lacks_core(&host));
            assert!(matches!(
                view.core_terminals.hub_of(&host).map(Hub::state),
                Some(State::Idle)
            ));
        });
    }
}

#[gpui::test]
fn a_terminal_asked_for_while_herdr_core_installs_waits_for_it(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        Fixture(cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)))
    });
    let view = fixture.update(cx, |fixture, _| fixture.0.clone());
    let (mut remote, _server) = connected_endpoint("ssh:fixture");
    remote.connection.target = ssh();
    // Every attempt holds until the test ends, as a bridge still dialing.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Mutex::new(held);
    let join = move |start| {
        let _ = held.lock().map(|held| held.recv());
        missing(start)
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints.push(remote);
            view.selected_endpoint = 1;
            view.reset_selected(cx);
            view.config.herdr_2_terminals = true;
            let host = Host::Remote(scope());
            view.core_terminals
                .set_remote_hub_for_test(scope(), ssh(), Hub::with_join(join));
            view.command(Command::NewCoreTerminal, window, cx);
            assert!(view.core_terminals.waits_on_for_test(&host));
            view.core_install_ended(
                host.clone(),
                "fixture",
                Ok(sheprd_remote::core_install::Outcome::Installed),
                cx,
            );
            // The terminal keeps its place, and the core is dialed anew.
            assert!(view.core_terminals.waits_on_for_test(&host));
            assert!(matches!(
                view.core_terminals.hub_of(&host).map(Hub::state),
                Some(State::Connecting(_))
            ));
        })
    });
    drop(release);
}

#[gpui::test]
fn the_window_that_installed_dials_the_host_once(cx: &mut gpui::TestAppContext) {
    fn installed(
        _: &ConnectTarget,
    ) -> std::result::Result<
        sheprd_remote::core_install::Outcome,
        crate::core_terminals::install::Failed,
    > {
        Ok(sheprd_remote::core_install::Outcome::Installed)
    }
    // The window is the root view, as the app's are, so the install's
    // pass over every window reaches it.
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let (mut remote, _server) = connected_endpoint("ssh:fixture");
    remote.connection.target = ssh();
    // Every attempt holds until the test ends, as a bridge still dialing.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Mutex::new(held);
    let join = move |start| {
        let _ = held.lock().map(|held| held.recv());
        missing(start)
    };
    let host = Host::Remote(scope());
    view.update_in(cx, |view, window, cx| {
        view.endpoints.push(remote);
        view.selected_endpoint = 1;
        view.reset_selected(cx);
        view.config.herdr_2_terminals = true;
        view.core_terminals
            .set_remote_hub_for_test(scope(), ssh(), Hub::with_join(join));
        view.command(Command::NewCoreTerminal, window, cx);
        view.install_core_with("ssh:fixture", installed, cx);
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        let hub = view.core_terminals.hub_of(&host).unwrap();
        // The terminal's own attempt, then one for the installed core.
        assert_eq!(hub.dials(), 2);
        assert!(view.core_terminals.waits_on_for_test(&host));
    });
    drop(release);
}
