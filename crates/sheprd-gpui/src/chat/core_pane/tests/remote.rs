use super::*;
use crate::{browser::Scope, core_terminals::Host};

#[cfg(feature = "cloud")]
#[test]
fn cloud_chat_identity_uses_the_provider_account_and_machine_id_not_its_label() {
    for provider in sheprd_client::CloudProvider::ALL {
        let host = Host::Remote(Scope::endpoint("cloud:saved"));
        let target = |id: &str| sheprd_client::ConnectTarget::Cloud {
            provider: *provider,
            account: "account-one".into(),
            id: id.into(),
            machine: "same-display-name".into(),
            session: "default".into(),
        };
        let first = crate::chat::discovery::remote_route(&target("first-id")).unwrap();
        let second = crate::chat::discovery::remote_route(&target("second-id")).unwrap();
        let terminal = info("t1", Some(A));
        let first = target_on(&host, &first, &terminal).unwrap();
        let second = target_on(&host, &second, &terminal).unwrap();
        assert_eq!(first.pane_id, second.pane_id);
        assert_ne!(first, second);
        assert!(!first.is_local());
    }
}

#[test]
fn matching_ids_and_endpoint_tokens_on_different_hosts_remain_distinct() {
    let terminals = [info("t1", Some(A))];
    let local = targets(&terminals).remove(0);
    let host = Host::Remote(Scope::endpoint("ssh:box"));
    let remote = targets_on(&host, &Route::Ssh("box".into()), &terminals).remove(0);
    assert_ne!(local.pane_id, remote.pane_id);
    assert_eq!(remote.route, Route::Ssh("box".into()));
    assert_eq!(
        remote.surface,
        Surface::RemoteCore {
            host,
            terminal: terminals[0].id.clone()
        }
    );
    assert!(terminal_of(&remote.pane_id).is_none());
    assert_eq!(with_core(vec![local], vec![remote]).len(), 2);
}

#[test]
fn remote_labels_cannot_name_arbitrary_paths_or_forwarded_terminals() {
    let host = Host::Remote(Scope::endpoint("ssh:box"));
    let terminals = [
        info("t1", Some("../../other.sock")),
        info("another/t1", Some(A)),
    ];
    assert!(targets_on(&host, &Route::Ssh("box".into()), &terminals).is_empty());
}

#[test]
fn an_ended_chat_or_recovered_shell_cannot_retain_the_old_overlay() {
    let host = Host::Remote(Scope::endpoint("ssh:box"));
    let route = Route::Ssh("box".into());
    let mut terminal = info("t1", Some(A));
    terminal.foreground = None;
    assert!(targets_on(&host, &route, [&terminal]).is_empty());
    terminal.foreground = Some(sheprd_backend::Process {
        name: "bash".into(),
        pid: 42,
        ..Default::default()
    });
    assert!(targets_on(&host, &route, [&terminal]).is_empty());
    terminal.foreground.as_mut().unwrap().name = "sheprd-acp".into();
    terminal.state = RunState::Lost;
    assert!(targets_on(&host, &route, [&terminal]).is_empty());
}

#[test]
fn wsl_targets_use_the_distribution_route_not_the_windows_endpoint_directory() {
    let host = Host::Remote(Scope::endpoint("wsl:Ubuntu"));
    let route = Route::Remote(sheprd_client::ConnectTarget::Wsl {
        distro: "Ubuntu".into(),
        session: "default".into(),
    });
    let targets = targets_on(&host, &route, &[info("t1", Some(A))]);
    assert_eq!(targets[0].route, route);
    assert!(!targets[0].is_local());
}

#[test]
fn remote_keys_keep_scope_boundaries_unambiguous() {
    let a = key_on(
        &Host::Remote(Scope::endpoint("ssh:a:t")),
        &TerminalId::new("1").unwrap(),
    );
    let b = key_on(
        &Host::Remote(Scope::endpoint("ssh:a")),
        &TerminalId::new("t:1").unwrap(),
    );
    assert_ne!(a, b);
}

#[test]
fn a_remote_launch_finds_the_hosts_program_and_preserves_endpoint_ownership() {
    let endpoint: EndpointId = A.parse().unwrap();
    let extra = BTreeMap::from([
        (ENDPOINT_ENV.into(), B.into()),
        (PANE_ENV.into(), "w1:p99".into()),
        ("GH_HOST".into(), "company.test".into()),
    ]);
    let request = remote_spawn(Agent::Codex, &endpoint, &extra);
    assert_eq!(&request.argv[..2], ["/bin/sh", "-c"]);
    assert_eq!(
        request.argv[2],
        super::super::super::remote_program_script("codex")
    );
    assert_eq!(request.labels.get(TOKEN), Some(&A.to_owned()));
    assert!(request.env.contains(&(ENDPOINT_ENV.into(), A.into())));
    assert!(request.env.contains(&(PANE_ENV.into(), String::new())));
    assert!(
        request
            .env
            .contains(&("GH_HOST".into(), "company.test".into()))
    );
    assert!(request.cwd.is_none());
}
