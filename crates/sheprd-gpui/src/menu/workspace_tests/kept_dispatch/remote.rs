use super::*;

#[gpui::test]
fn a_delayed_catalog_arrival_rechecks_identity_before_landing(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut endpoint = view.endpoints.remove(1);
            let identity = crate::teleport::host_for(&endpoint.connection.target)
                .unwrap()
                .identity();
            view.dispatch_job = Some(Job::finished(
                "Box",
                "app",
                Ok(Dispatched::Kept {
                    endpoint_id: endpoint.id.clone(),
                    host_identity: identity,
                    path: NEW.into(),
                    label: None,
                    checkout: None,
                }),
            ));
            view.poll_dispatch(window, cx);
            assert!(
                view.dispatch_job.is_some(),
                "creation waits for the catalog"
            );
            assert!(view.kept_workspace_at(Path::new(NEW)).is_none());
            endpoint.connection.target = sheprd_client::ConnectTarget::Ssh {
                target: "different@host.invalid".into(),
                session: "default".into(),
            };
            view.endpoints.push(endpoint);
            view.poll_dispatch(window, cx);
            assert!(view.dispatch_job.is_none());
            assert_eq!(view.selected_endpoint, 0);
            assert!(view.kept_workspace_at(Path::new(NEW)).is_none());
        });
    });
}

#[gpui::test]
fn remote_routing_uses_that_hosts_availability_not_the_selected_hosts(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, true);
    view.update(cx, |view, _| {
        view.config.herdr_2_terminals = false;
        assert!(!view.dispatch_keeps("ssh:box"));
        view.endpoints[1].live.snapshot = None;
        view.endpoints[1].live.ssh_failure = Some(sheprd_client::SshFailure::HerdrMissing);
        assert!(view.dispatch_keeps("ssh:box"));
        let destination = view.dispatch_destination("ssh:box").unwrap();
        assert_eq!(destination.lands_in, sheprd_session::Keeper::App);
        assert!(destination.repositories.unwrap().is_empty());
        assert!(!view.dispatch_keeps("removed-host"));
        view.endpoints[1].enabled = false;
        assert!(view.dispatch_destination("ssh:box").is_none());
    });
}

#[gpui::test]
fn a_changed_host_never_opens_the_dispatched_path_locally(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let identity = crate::teleport::host_for(&view.endpoints[1].connection.target)
                .unwrap()
                .identity();
            view.endpoints[1].connection.target = sheprd_client::ConnectTarget::Ssh {
                target: "somebody@different.invalid".into(),
                session: "default".into(),
            };
            view.dispatch_job = Some(Job::finished(
                "Box",
                "app",
                Ok(Dispatched::Kept {
                    endpoint_id: "ssh:box".into(),
                    host_identity: identity,
                    path: NEW.into(),
                    label: None,
                    checkout: None,
                }),
            ));
            view.poll_dispatch(window, cx);
            assert_eq!(view.selected_endpoint, 0);
            assert!(view.kept_workspace_at(Path::new(NEW)).is_none());
            assert!(view.queued_kept_setup().is_none());
        });
    });
}

#[gpui::test]
fn a_remote_dispatch_lands_on_its_host_and_queues_its_setup(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let target = view.endpoints[1].connection.target.clone();
            let identity = crate::teleport::host_for(&target).unwrap().identity();
            let scope = crate::browser::scope(&view.endpoints[1]);
            view.core_terminals.set_remote_hub_for_test(
                scope,
                target,
                crate::core_terminals::Hub::with_join(|_| {
                    Err(crate::core_terminals::Error::Bridge(
                        sheprd_client::core_bridge::Error::Missing.into(),
                    ))
                }),
            );
            view.dispatch_job = Some(Job::finished(
                "Box",
                "app",
                Ok(Dispatched::Kept {
                    endpoint_id: "ssh:box".into(),
                    host_identity: identity,
                    path: NEW.into(),
                    label: Some("Remote lane".into()),
                    checkout: Some(crate::teleport::NewCheckout {
                        repo_key: KEY.into(),
                        path: NEW.into(),
                        root: Some("/src/app".into()),
                    }),
                }),
            ));
            view.poll_dispatch(window, cx);
            assert_eq!(view.selected_endpoint, 1);
            assert!(view.kept_workspace_at(Path::new(NEW)).is_some());
            assert_eq!(view.queued_kept_setup().unwrap().endpoint_id, "ssh:box");
            assert!(
                view.sessions(0)
                    .core
                    .unwrap()
                    .workspaces
                    .iter()
                    .all(|workspace| workspace.cwd != NEW)
            );
        });
    });
}
