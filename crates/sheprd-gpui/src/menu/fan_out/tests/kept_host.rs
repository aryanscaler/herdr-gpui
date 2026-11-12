use super::*;

#[gpui::test]
fn a_kept_origin_runs_its_lanes_on_the_host_that_keeps_it(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, _| {
        let source = Source {
            workspace_id: "space-1".into(),
            repo_key: "/same/path/.git".into(),
            repo_label: "app".into(),
            base: "HEAD".into(),
            checkout: Some("/same/path".into()),
        };
        assert!(
            view.fan_out_host(&source).unwrap().is_local(),
            "a custom Herdr socket still keeps local core files"
        );
        for target in [
            sheprd_client::ConnectTarget::Ssh {
                target: "box".into(),
                session: "work".into(),
            },
            sheprd_client::ConnectTarget::Wsl {
                distro: "Ubuntu".into(),
                session: "work".into(),
            },
        ] {
            view.endpoints[0].connection.target = target;
            // Lanes run through that host's scripts, never this computer's.
            assert!(!view.fan_out_host(&source).unwrap().is_local());
        }
    });
}
