use super::{Action, Bridge, Data, Feed, Menu, action};
use async_channel::bounded;

#[test]
fn each_message_becomes_its_action() {
    let of = |key: &'static str| action(&Data::map([(key, "x".into())]));
    assert_eq!(of("command"), Some(Action::Command("x".into())));
    assert_eq!(
        of("focus_workspace"),
        Some(Action::FocusWorkspace {
            endpoint: None,
            workspace: "x".into()
        })
    );
    assert_eq!(
        of("focus_pane"),
        Some(Action::FocusPane {
            endpoint: None,
            pane: "x".into()
        })
    );
    assert_eq!(of("panel"), Some(Action::Panel("x".into())));
    assert_eq!(of("toggle_device"), Some(Action::ToggleDevice("x".into())));
    assert_eq!(of("select_device"), Some(Action::SelectDevice("x".into())));
}

#[test]
fn a_row_on_another_device_names_it() {
    let message = Data::map([
        ("menu", "workspace".into()),
        ("workspace", "w1".into()),
        ("endpoint", "studio".into()),
    ]);
    assert_eq!(
        action(&message),
        Some(Action::Menu(Menu::Workspace {
            endpoint: Some("studio".into()),
            workspace: "w1".into()
        }))
    );
    let fold = Data::map([
        ("toggle_repository", "repo".into()),
        ("endpoint", "studio".into()),
    ]);
    assert_eq!(
        action(&fold),
        Some(Action::ToggleRepository {
            endpoint: Some("studio".into()),
            key: "repo".into()
        })
    );
    assert_eq!(
        action(&Data::map([("fold_all", true.into())])),
        Some(Action::FoldAll(true))
    );
    // A device's menu needs the device.
    assert_eq!(action(&Data::map([("menu", "device".into())])), None);
}

#[test]
fn a_message_asking_for_nothing_known_is_no_action() {
    assert_eq!(action(&Data::map([("other", "x".into())])), None);
    assert_eq!(action(&Data::map([("command", 3.into())])), None);
}

#[test]
fn a_full_queue_drops_an_action_without_blocking() {
    let (actions, received) = bounded(1);
    let bridge = Bridge {
        actions,
        feed: Feed::default(),
    };
    let message = || Data::map([("command", "x".into())]);
    for _ in 0..3 {
        if let Some(action) = action(&message()) {
            let _ = bridge.actions.try_send(action);
        }
    }
    assert_eq!(received.len(), 1);
}

#[test]
fn state_a_provider_not_started_yet_is_offered_again() {
    let feed = Feed::default();
    let mut last = None;
    assert!(!feed.offer(&mut last, serde_json::json!({ "a": 1 })));
    assert_eq!(last, None);
}

#[test]
fn state_already_published_is_not_published_again() {
    let feed = Feed::default();
    let state = serde_json::json!({ "a": 1 });
    let mut last = Some(state.clone());
    assert!(feed.offer(&mut last, state.clone()));
    assert_eq!(last, Some(state));
}

#[test]
fn a_dropped_row_and_a_dragged_edge_become_their_actions() {
    let dropped = Data::map([
        ("move_workspace", "c".into()),
        ("target", "b".into()),
        ("after", true.into()),
    ]);
    assert_eq!(
        action(&dropped),
        Some(Action::MoveWorkspace {
            workspace: "c".into(),
            target: "b".into(),
            after: true
        })
    );
    // A move needs its target.
    assert_eq!(action(&Data::map([("move_workspace", "c".into())])), None);
    let edge = |phase: &str, clicks: f64| {
        action(&Data::map([
            ("sidebar_width", true.into()),
            ("phase", phase.into()),
            ("dx", 12.5.into()),
            ("clicks", clicks.into()),
        ]))
    };
    assert_eq!(
        edge("down", 2.0),
        Some(Action::SidebarWidth {
            phase: super::Drag::Down { reset: true },
            dx: 12.5
        })
    );
    assert_eq!(
        edge("move", 1.0),
        Some(Action::SidebarWidth {
            phase: super::Drag::Move,
            dx: 12.5
        })
    );
    assert_eq!(edge("hover", 1.0), None);
}

#[test]
fn a_plain_launch_opens_the_luau_window_and_commands_stay_the_apps() {
    let args = |list: &[&str]| -> Vec<std::ffi::OsString> { list.iter().map(Into::into).collect() };
    assert!(super::wanted(&args(&[])));
    assert!(super::wanted(&args(&["--check"])));
    assert!(!super::wanted(&args(&["browser", "open", "x.html"])));
    assert!(!super::wanted(&args(&["--socket", "/tmp/c.sock"])));
    assert!(!super::wanted(&args(&["--session", "work"])));
}
