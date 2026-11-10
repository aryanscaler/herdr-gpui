use super::*;

#[test]
fn title_names_the_agent_and_what_it_wants() {
    assert_eq!(
        title(SemanticNotificationKind::NeedsAttention, "Claude"),
        "\u{2068}Claude\u{2069} needs you"
    );
    assert_eq!(
        title(SemanticNotificationKind::Finished, "Codex"),
        "\u{2068}Codex\u{2069} finished"
    );
}

#[test]
fn body_shows_the_task_over_the_place() {
    assert_eq!(
        text(Some("Fix login"), "Claude", Some("api · tests")).as_deref(),
        Some("Fix login\napi · tests")
    );
    // A title that only repeats the agent's name says nothing more.
    assert_eq!(
        text(Some(" Claude "), "Claude", Some("api")).as_deref(),
        Some("api")
    );
    assert_eq!(
        text(Some("Fix login"), "Claude", None).as_deref(),
        Some("Fix login")
    );
    assert_eq!(text(None, "Claude", None), None);
    // Terminal titles are untrusted: controls are dropped.
    assert_eq!(
        text(Some("a\u{1b}[31mb"), "Claude", None).as_deref(),
        Some("a[31mb")
    );
}

#[test]
fn one_tag_per_terminal() {
    let terminal = TerminalId::new("t-1").unwrap();
    assert_eq!(tag(&terminal), "herdr-core:t-1");
}

fn posted(system: bool, finished: bool, kind: SemanticNotificationKind) -> Vec<String> {
    let mut cx = gpui::TestAppContext::single();
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|_, cx| super::super::install_registry(cx));
    let terminal = TerminalId::new("t-7").unwrap();
    cx.update(|window, cx| {
        let handle = window.window_handle().downcast::<SheprdWindow>().unwrap();
        view.update(cx, |view, cx| {
            view.config.notifications.enabled = !system;
            view.config.notifications.system = system;
            view.config.notifications.finished = finished;
            view.post_core_notification(handle, &terminal, kind, cx);
        });
    });
    cx.run_until_parked();
    cx.shown_system_notifications()
        .into_iter()
        .map(|shown| format!("{}|{}|{}", shown.tag, shown.title, shown.body))
        .collect()
}

#[test]
fn a_herdr_2_agent_posts_only_with_system_delivery() {
    assert_eq!(
        posted(true, true, SemanticNotificationKind::NeedsAttention),
        ["herdr-core:t-7|\u{2068}Agent\u{2069} needs you|"]
    );
    assert!(posted(false, true, SemanticNotificationKind::NeedsAttention).is_empty());
    // Finished notices can be turned off on their own.
    assert!(posted(true, false, SemanticNotificationKind::Finished).is_empty());
    assert_eq!(
        posted(true, true, SemanticNotificationKind::Finished).len(),
        1
    );
}

#[test]
fn a_click_on_a_herdr_2_notification_is_routed_to_its_window() {
    let mut cx = gpui::TestAppContext::single();
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|_, cx| super::super::install_registry(cx));
    let terminal = TerminalId::new("t-8").unwrap();
    cx.update(|window, cx| {
        let handle = window.window_handle().downcast::<SheprdWindow>().unwrap();
        view.update(cx, |view, cx| {
            view.config.notifications.enabled = false;
            view.config.notifications.system = true;
            view.post_core_notification(handle, &terminal, SemanticNotificationKind::Finished, cx);
        });
    });
    // The click goes to the agent's terminal, not a Herdr 1 notice.
    cx.update(|_, cx| {
        let registry = cx.global::<Registry>();
        let posted = registry.find("herdr-core:t-8").unwrap();
        assert!(matches!(&posted.target, Target::Core(id) if *id == terminal));
        assert!(!super::super::open("herdr-core:t-9", cx));
    });
    // A click on it with no tab holding the agent leaves the window as is.
    cx.simulate_system_notification_response(gpui::SystemNotificationResponse {
        tag: "herdr-core:t-8".into(),
        action_id: None,
    });
    cx.run_until_parked();
}

#[test]
fn a_banner_names_the_agent_another_window_holds() {
    use sheprd_backend::{AgentStatus, Process, RunState, TerminalInfo};
    let mut cx = gpui::TestAppContext::single();
    let terminal = TerminalId::new("t5").unwrap();
    let info = TerminalInfo {
        agent: AgentStatus::Blocked,
        cwd: None,
        foreground: Some(Process {
            argv: vec!["claude".into()],
            name: "claude".into(),
            pid: 9,
        }),
        id: terminal.clone(),
        labels: Default::default(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: Some("Fix the login".into()),
    };
    let holder = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    holder
        .update(&mut cx, |view, _, _| {
            view.core_terminals.show_for_test(
                crate::browser::TabId::test(2),
                crate::core_terminals::panes::Panes::single(terminal.clone()),
                false,
                vec![info],
            );
        })
        .unwrap();
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|_, cx| super::super::install_registry(cx));
    cx.update(|window, cx| {
        let handle = window.window_handle().downcast::<SheprdWindow>().unwrap();
        view.update(cx, |view, cx| {
            view.config.notifications.enabled = false;
            view.config.notifications.system = true;
            view.post_core_notification(
                handle,
                &terminal,
                SemanticNotificationKind::NeedsAttention,
                cx,
            );
        });
    });
    let shown = cx.shown_system_notifications();
    let [shown] = shown.as_slice() else {
        panic!("expected one notification: {shown:?}");
    };
    assert_eq!(shown.title, "\u{2068}claude\u{2069} needs you");
    assert!(shown.body.starts_with("Fix the login"), "{}", shown.body);
    // A click goes to the window holding it.
    cx.update(|_, cx| assert!(super::super::open("herdr-core:t5", cx)));
}
