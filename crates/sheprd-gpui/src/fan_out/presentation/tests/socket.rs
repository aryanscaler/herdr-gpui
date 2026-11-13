use super::*;

/// A kept workspace behind a custom Herdr socket launches its lanes on this
/// machine, so their terminals are shown there rather than dropped.
#[gpui::test]
fn a_lane_from_a_kept_workspace_behind_a_custom_socket_is_shown(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.endpoints.push(crate::endpoint::Endpoint::new(
                "socket:fanout".into(),
                "Socket".into(),
                ConnectTarget::Socket("/tmp/fanout-test.sock".into()),
                true,
            ));
            let host =
                sheprd_remote::work::host_for_kept(&view.endpoints[1].connection.target).unwrap();
            let scope = view.fan_out_scope("socket:fanout").unwrap();
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t-lane").unwrap()), false);
            view.show_fan_out_terminal("socket:fanout", &host, "lane", "/work/lane", location, cx);
            let id = view
                .kept_workspace_in(&scope, Path::new("/work/lane"), cx)
                .unwrap();
            assert_eq!(cx.global::<Store>().in_workspace(&scope, &id).count(), 1);
        })
    });
}
