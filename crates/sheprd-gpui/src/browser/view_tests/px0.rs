use super::*;

#[gpui::test]
fn a_window_with_stale_settings_cannot_reset_the_shared_workbench(cx: &mut gpui::TestAppContext) {
    use crate::px0::{Config, Launcher, State};
    let (view, cx) = window(cx);
    let current = WebUrl::try_from("http://localhost:8888/").unwrap();
    cx.update(|_, cx| {
        Launcher::fixture(
            cx,
            Config {
                url: Some(current.clone()),
            },
            State::Ready(current.clone()),
        )
    });
    for old in ["http://localhost:7777/", "http://localhost:9999/"] {
        view.update(cx, |view, _| {
            view.config.px0.url = Some(WebUrl::try_from(old).unwrap())
        });
        cx.update(|window, cx| view.update(cx, |view, cx| view.ensure_px0_page(window, cx)));
        cx.update(|_, cx| {
            assert!(matches!(Launcher::state(cx), State::Ready(url) if url == current))
        });
    }
}

#[gpui::test]
fn px0_opens_in_the_active_group_without_browser_chrome(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    view.update(cx, |view, _| {
        let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
        snapshot.panes = serde_json::from_value(serde_json::json!([{
            "pane_id": "p0", "workspace_id": "w0", "tab_id": "t0", "label": "shell",
            "cwd": if cfg!(windows) { r"C:\checkout" } else { "/checkout" },
            "foreground_cwd": null, "focused": true, "right_click_passthrough": false
        }]))
        .unwrap();
    });
    let id = cx.update(|window, cx| view.update(cx, |view, cx| view.open_px0(None, window, cx)));
    if !super::super::EMBEDDED {
        assert!(id.is_none());
        return;
    }
    let id = id.unwrap();
    draw(cx);
    assert!(cx.debug_bounds("px0-placeholder").is_some());
    assert!(cx.debug_bounds("browser-reload").is_none());
    let again = cx.update(|window, cx| view.update(cx, |view, cx| view.open_px0(None, window, cx)));
    assert_eq!(again, Some(id));
    assert!(cx.update(|_, cx| view.read(cx).group_shows_page(id, cx)));
    let first = view.read_with(cx, |view, _| view.group_slots()[0].id);
    cx.update(|window, cx| view.update(cx, |view, cx| view.split_group(first, window, cx)));
    let second = view.read_with(cx, |view, _| view.group_slots()[1].id);
    let moved =
        cx.update(|window, cx| view.update(cx, |view, cx| view.open_px0(Some(second), window, cx)));
    assert_eq!(moved, Some(id));
    assert_eq!(
        cx.update(|_, cx| view.read(cx).group_shown(second, cx)),
        crate::browser::Shown::Page(id)
    );
}

fn started_in(view: &mut SheprdWindow, cwd: &str) {
    let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
    snapshot.panes = serde_json::from_value(serde_json::json!([{
        "pane_id": "p0", "workspace_id": "w0", "tab_id": "t0", "label": "shell",
        "cwd": cwd, "foreground_cwd": null, "focused": true, "right_click_passthrough": false
    }]))
    .unwrap();
}

#[gpui::test]
fn other_remote_workspaces_never_open_a_local_px0(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    view.update(cx, |view, _| {
        started_in(view, "/srv/checkout");
        view.endpoints[0].connection.target = sheprd_client::ConnectTarget::Wsl {
            distro: "Ubuntu".into(),
            session: "default".into(),
        };
        assert!(view.px0_target().is_none());
    });
    assert!(
        cx.update(|window, cx| view.update(cx, |view, cx| view.open_px0(None, window, cx)))
            .is_none()
    );
}

/// Unix only: Windows has no `ssh -L` to forward a host's workbench.
#[cfg(unix)]
#[gpui::test]
fn an_ssh_workspace_opens_its_hosts_own_workbench(cx: &mut gpui::TestAppContext) {
    use crate::browser::px0::Px0Host;
    use crate::px0::{Hosts, State};
    let (view, cx) = window(cx);
    view.update(cx, |view, _| {
        started_in(view, "/srv/checkout");
        view.endpoints[0].connection.target = sheprd_client::ConnectTarget::Ssh {
            target: "dev@build".into(),
            session: "default".into(),
        };
        assert_eq!(
            view.px0_target(),
            Some((Px0Host::Ssh("dev@build".into()), "/srv/checkout".into()))
        );
    });
    let base = WebUrl::try_from("http://127.0.0.1:41234/").unwrap();
    // The local workbench must not serve the host's folder, whatever it reports.
    cx.update(|_, cx| {
        crate::px0::Launcher::fixture(
            cx,
            crate::px0::Config::default(),
            State::Ready(WebUrl::try_from("http://127.0.0.1:7777/").unwrap()),
        );
        Hosts::fixture(cx, [("dev@build".to_owned(), State::Starting)]);
    });
    let id = cx.update(|window, cx| view.update(cx, |view, cx| view.open_px0(None, window, cx)));
    if !super::super::EMBEDDED {
        assert!(id.is_none());
        return;
    }
    let id = id.unwrap();
    let location = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            super::super::view::store(cx)
                .unwrap()
                .get(id)
                .unwrap()
                .location
                .clone()
        })
    };
    cx.update(|window, cx| view.update(cx, |view, cx| view.ensure_px0_page(window, cx)));
    assert_eq!(location(cx), None, "nothing loads while the host's starts");
    cx.update(|_, cx| Hosts::fixture(cx, [("dev@build".to_owned(), State::Ready(base.clone()))]));
    cx.update(|window, cx| view.update(cx, |view, cx| view.ensure_px0_page(window, cx)));
    let expected = crate::px0::workspace_url(&base, "/srv/checkout").unwrap();
    assert_eq!(location(cx), Some(Location::Web { url: expected }));
    // Retry on another host leaves this page; on its own host it drops it,
    // as the workbench comes back at a new address.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.px0_bound_tabs(), [id]);
            view.restart_px0(&Px0Host::Ssh("dev@other".into()), cx);
            assert_eq!(view.px0_bound_tabs(), [id]);
            view.restart_px0(&Px0Host::Ssh("dev@build".into()), cx);
            assert!(view.px0_bound_tabs().is_empty());
            assert!(matches!(Hosts::state(cx, "dev@build"), State::Idle));
        });
    });
}
