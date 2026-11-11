use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn the_state_lists_each_workspace_with_what_its_row_shows(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let state = view.update(cx, |view, cx| {
        view.live.snapshot = Some(Arc::new(crate::sidebar::layout_tests::snapshot(3)));
        view.hosted_state(cx)
    });
    let workspaces = state.main["workspaces"].as_array().unwrap();
    assert_eq!(workspaces.len(), 3);
    assert!(workspaces[0]["id"].is_string());
    assert!(workspaces[0]["label"].is_string());
    assert!(
        state.main["panels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|panel| panel["id"] == "workspaces")
    );
}

#[gpui::test]
fn an_unknown_command_or_panel_changes_nothing(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update_in(cx, |view, window, cx| {
        let before = view.hosted_state(cx);
        view.hosted_action(Action::Command("no-such-command".into()), window, cx);
        view.hosted_action(Action::Panel("no-such-panel".into()), window, cx);
        assert_eq!(view.hosted_state(cx), before);
    });
}

#[test]
fn every_panel_has_its_own_id() {
    let mut ids: Vec<_> = ActivityPanel::ALL.iter().copied().map(panel_id).collect();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count);
}

#[gpui::test]
fn a_stamp_changes_with_the_snapshot_and_not_with_a_redraw(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, cx| {
        view.live.snapshot = Some(Arc::new(crate::sidebar::layout_tests::snapshot(2)));
        let before = view.hosted_stamp();
        cx.notify();
        assert!(view.hosted_stamp() == before);
        view.live.snapshot = Some(Arc::new(crate::sidebar::layout_tests::snapshot(2)));
        assert!(view.hosted_stamp() != before);
    });
}

#[gpui::test]
fn a_stamp_changes_when_the_sidebar_hides(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, _| {
        let before = view.hosted_stamp();
        view.sidebar_visible = !view.sidebar_visible;
        assert!(view.hosted_stamp() != before);
    });
}

#[test]
fn dark_backgrounds_are_told_from_light_ones() {
    assert!(is_dark(0x101419));
    assert!(!is_dark(0xf6f6f8));
}

#[gpui::test]
fn linked_worktrees_follow_their_parent_as_children(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let state = view.update(cx, |view, cx| {
        let mut snapshot = crate::sidebar::layout_tests::snapshot(3);
        let tree = |linked| sheprd_client::protocol::ClientShellWorktree {
            is_linked_worktree: linked,
            key: "repo".into(),
            label: "repo".into(),
        };
        snapshot.workspaces[0].worktree = Some(tree(true));
        snapshot.workspaces[1].worktree = Some(tree(false));
        snapshot.workspaces[2].worktree = None;
        view.live.snapshot = Some(Arc::new(snapshot));
        view.hosted_state(cx)
    });
    let rows = state.main["workspaces"].as_array().unwrap();
    assert_eq!(rows[0]["child"], false);
    assert_eq!(rows[0]["linked"], false);
    assert_eq!(rows[1]["child"], true);
    assert_eq!(rows[1]["linked"], true);
}

#[gpui::test]
fn the_layout_and_its_spacing_follow_the_config(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, cx| {
        let before = view.hosted_stamp();
        view.config.layout.mode = crate::config::LayoutMode::Orca;
        assert!(view.hosted_stamp() != before);
        let layout = &view.hosted_state(cx).main["layout"];
        assert_eq!(layout["name"], "orca");
        assert_eq!(layout["outline"], true);
        assert_eq!(layout["title_case"], true);
    });
}

#[gpui::test]
fn agents_are_labelled_and_sorted_as_the_agents_list_shows_them(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let state = view.update(cx, |view, cx| {
        let mut snapshot = crate::sidebar::layout_tests::snapshot(2);
        snapshot.agents[1].agent_status = AgentStatus::Blocked;
        view.live.snapshot = Some(Arc::new(snapshot));
        view.agent_sort = AgentSort::Priority;
        view.hosted_state(cx)
    });
    assert_eq!(state.main["agent_sort"], "priority");
    let agents = state.main["agents"].as_array().unwrap();
    // A blocked agent rises above a working one.
    assert_eq!(agents[0]["pane"], "p1");
    assert_eq!(agents[0]["status"], "blocked");
    let first = &agents[1];
    assert_eq!(first["name"], "Claude Code");
    assert_eq!(first["kind"], "claude");
    assert_eq!(first["workspace_label"], "herdr");
    assert_eq!(first["focused"], false);
}

#[gpui::test]
fn each_panel_says_whether_its_count_is_unseen(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let state = view.update(cx, |view, cx| view.hosted_state(cx));
    for panel in state.main["panels"].as_array().unwrap() {
        assert!(panel["unseen"].is_boolean(), "{panel}");
    }
}

#[gpui::test]
fn folding_every_group_hides_its_worktrees_under_their_checkout(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let rows = |view: &mut SheprdWindow, cx: &mut gpui::Context<SheprdWindow>| {
        let state = view.hosted_state(cx);
        state.main["devices"][0]["workspaces"]
            .as_array()
            .unwrap()
            .clone()
    };
    view.update_in(cx, |view, window, cx| {
        let mut snapshot = crate::sidebar::layout_tests::snapshot(3);
        let tree = |linked| sheprd_client::protocol::ClientShellWorktree {
            is_linked_worktree: linked,
            key: "repo".into(),
            label: "repo".into(),
        };
        snapshot.workspaces[0].worktree = Some(tree(false));
        snapshot.workspaces[1].worktree = Some(tree(true));
        view.live.snapshot = Some(Arc::new(snapshot));
        let open = rows(view, cx);
        assert_eq!(open.len(), 3);
        assert_eq!(open[0]["group"], "repo");
        assert_eq!(open[0]["collapsed"], false);
        assert_eq!(open[1]["child"], true);

        let before = view.hosted_stamp();
        view.hosted_action(Action::FoldAll(true), window, cx);
        assert!(view.hosted_stamp() != before);
        let folded = rows(view, cx);
        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0]["collapsed"], true);

        view.hosted_action(
            Action::ToggleRepository {
                endpoint: None,
                key: "repo".into(),
            },
            window,
            cx,
        );
        assert_eq!(rows(view, cx).len(), 3);
    });
}

#[gpui::test]
fn each_row_names_its_device(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let state = view.update(cx, |view, cx| {
        view.live.snapshot = Some(Arc::new(crate::sidebar::layout_tests::snapshot(2)));
        view.hosted_state(cx)
    });
    let device = &state.main["devices"][0];
    assert_eq!(device["id"], crate::endpoint::LOCAL);
    assert_eq!(device["workspaces"][0]["endpoint"], crate::endpoint::LOCAL);
    assert_eq!(state.main["agents"][0]["endpoint"], crate::endpoint::LOCAL);
    assert!(state.main["build"]["worktree"].is_boolean());
}

#[gpui::test]
fn hiding_agents_hides_the_agents_list(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, cx| {
        view.config.show_agents = true;
        view.config.agents_placement = crate::config::AgentsPlacement::Panel;
        assert_eq!(view.hosted_state(cx).main["agents_list"], true);
        let before = view.hosted_stamp();
        view.config.show_agents = false;
        assert!(view.hosted_stamp() != before);
        assert_eq!(view.hosted_state(cx).main["agents_list"], false);
        // Agents under their workspaces leave no list of their own.
        view.config.show_agents = true;
        view.config.agents_placement = crate::config::AgentsPlacement::Nested;
        assert_eq!(view.hosted_state(cx).main["agents_list"], false);
    });
}

#[gpui::test]
fn a_folded_group_shows_its_most_urgent_status_when_statuses_are_inline(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update_in(cx, |view, window, cx| {
        let mut snapshot = crate::sidebar::layout_tests::snapshot(3);
        let tree = |linked| sheprd_client::protocol::ClientShellWorktree {
            is_linked_worktree: linked,
            key: "repo".into(),
            label: "repo".into(),
        };
        snapshot.workspaces[0].worktree = Some(tree(false));
        snapshot.workspaces[0].agent_status = AgentStatus::Idle;
        snapshot.workspaces[1].worktree = Some(tree(true));
        snapshot.workspaces[1].agent_status = AgentStatus::Blocked;
        view.live.snapshot = Some(Arc::new(snapshot));
        view.hosted_action(Action::FoldAll(true), window, cx);
        let status = |view: &mut SheprdWindow, cx: &mut gpui::Context<SheprdWindow>| {
            view.hosted_state(cx).main["devices"][0]["workspaces"][0]["agent_status"].clone()
        };
        view.config.usage.inline = true;
        assert_eq!(status(view, cx), "blocked");
        view.config.usage.inline = false;
        assert_eq!(status(view, cx), "idle");
    });
}
