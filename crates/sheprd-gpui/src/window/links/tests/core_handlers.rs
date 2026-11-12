use super::*;
use crate::{
    core_terminals::{Host, LinkClick},
    link_handlers::{Handler, Handlers},
};
use sheprd_client::ConnectTarget;

fn click() -> LinkClick {
    LinkClick {
        handler_url: Some(URL.into()),
        target: RowTarget::Web(URL.into()),
        terminal: sheprd_backend::TerminalId::new("t1").unwrap(),
        cwd: None,
        alt: false,
    }
}

fn configure(view: &mut SheprdWindow, directory: &str, command: Vec<String>) {
    view.config.open_links_in = crate::config::LinkTarget::System;
    view.config.link_handlers = Handlers::compile(vec![Handler {
        platforms: None,
        action: None,
        config_directory: None,
        state_directory: None,
        plugin: "fixture".into(),
        id: "capture".into(),
        host: "local".into(),
        pattern: "^https://".into(),
        directory: directory.into(),
        command,
        enabled: true,
    }])
    .unwrap();
}

#[gpui::test]
fn unmatched_core_click_uses_normal_fallback(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.open_links_in = crate::config::LinkTarget::System;
            view.open_core_link(
                click(),
                &Host::Local,
                Some(&ConnectTarget::Local),
                window,
                cx,
            );
        })
    });
    assert_eq!(cx.opened_url().as_deref(), Some(URL));
}

#[gpui::test]
fn unsupported_manifest_platform_returns_to_normal_fallback(cx: &mut gpui::TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("herdr-plugin.toml"),
        r#"
id = "fixture"
platforms = []
[[actions]]
id = "open"
command = ["must-not-run"]
[[link_handlers]]
id = "web"
pattern = '^https://'
action = "open"
"#,
    )
    .unwrap();
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.open_links_in = crate::config::LinkTarget::System;
            view.config.link_plugins = vec![crate::link_handlers::Plugin {
                host: "local".into(),
                directory: folder.path().to_str().unwrap().into(),
                enabled: true,
                config_directory: None,
                state_directory: None,
            }];
            view.open_core_link(
                click(),
                &Host::Local,
                Some(&ConnectTarget::Local),
                window,
                cx,
            );
        })
    });
    cx.run_until_parked();
    assert_eq!(cx.opened_url().as_deref(), Some(URL));
}

#[gpui::test]
fn missing_route_spends_claimed_click(cx: &mut gpui::TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            configure(
                view,
                folder.path().to_str().unwrap(),
                vec!["never-started".into()],
            );
            view.open_core_link(click(), &Host::Local, None, window, cx);
            assert!(view.local_error.is_some());
            assert!(view.links.core_handler.is_none());
        })
    });
    assert!(cx.opened_url().is_none());
}

#[cfg(unix)]
#[gpui::test]
fn handled_and_failed_core_clicks_never_open_fallback(cx: &mut gpui::TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let (view, cx) = cx.add_window_view(fixture_window);
    for code in [0, 23] {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.local_error = None;
                configure(
                    view,
                    folder.path().to_str().unwrap(),
                    vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        format!("printf %s \"$HERDR_PLUGIN_CLICKED_URL\" > captured; exit {code}"),
                    ],
                );
                view.open_core_link(
                    click(),
                    &Host::Local,
                    Some(&ConnectTarget::Local),
                    window,
                    cx,
                );
            })
        });
        cx.run_until_parked();
        assert!(cx.opened_url().is_none());
        assert_eq!(
            std::fs::read_to_string(folder.path().join("captured")).unwrap(),
            URL
        );
        view.read_with(cx, |view, _| {
            assert!(view.links.core_handler.is_none());
            assert_eq!(view.local_error.is_some(), code != 0);
        });
    }
}
