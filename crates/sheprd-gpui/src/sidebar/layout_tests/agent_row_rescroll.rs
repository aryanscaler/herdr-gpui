//! Re-clicking an agents-list row scrolls its pane to the live bottom
//! instead of focusing it again: the classifier's decision, the guards
//! that keep navigation, and the wire the second click produces.

use super::*;
use crate::{
    navigation::{AgentRowAction, NavigationTarget, RescrollTarget},
    state::ConnectionStatus,
    window::MockPeer,
};
use sheprd_backend::AgentStatus as CoreAgentStatus;
use sheprd_session::{Keeper, Session, Workspace as CoreWorkspace};
use std::sync::Arc;

/// The pane the local row names, focused in the snapshot and in its agent,
/// so the row paints selected and the classifier agrees with the paint.
const PANE: &str = "p0";
const KEPT: &str = "kept";

/// The surface fixture for `listing`: an 80x24 frame showing `PANE`, the
/// size the window asked for.
fn fixture_surface(listing: &ClientShellSnapshot) -> PaneSurfaceFrame {
    PaneSurfaceFrame {
        boot_id: listing.boot_id.clone(),
        projection_revision: listing.revision,
        surface_revision: 1,
        frame: FrameData {
            width: 80,
            height: 24,
            cells: vec![],
            cursor: None,
            hyperlinks: vec![],
            graphics: vec![],
        },
        panes: vec![PaneSurfacePane {
            pane_id: PANE.into(),
            content_revision: 1,
            rect: SurfaceRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
            inner_rect: SurfaceRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
            scrollbar_rect: None,
            scroll: None,
            focused: true,
            mouse_reporting: true,
            sgr_pixel_mouse: false,
            alternate_screen_active: false,
            pixel_width: 800,
            pixel_height: 480,
        }],
        splits: vec![],
        popup: None,
        graphics: Default::default(),
    }
}

/// Local and `ssh:test` hosts, the local one ready for input on the mock
/// peer: its surface arrived, sized as the window asked, showing `PANE`.
fn rescroll_fixture(
    cx: &mut gpui::TestAppContext,
) -> (Entity<SheprdWindow>, MockPeer, &mut gpui::VisualTestContext) {
    let peer = MockPeer::advertising(&["pane.focus", "pane.scroll"]);
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            peer.prepare(&mut view, cx);
            // `prepare` detaches, which clears the surface request; a
            // coherent 80x24 surface needs the request flag back.
            view.endpoints[0].request_surface_for_test();
            let mut listing = snapshot(1);
            listing.agents[0].focused = true;
            listing.panes = vec![
                serde_json::from_value(serde_json::json!({
                    "pane_id": PANE, "workspace_id": "w0", "tab_id": "t0", "label": "review",
                    "cwd": "/repo", "foreground_cwd": "/repo", "focused": true,
                    "right_click_passthrough": false,
                }))
                .unwrap(),
            ];
            listing.focused_pane_id = Some(PANE.into());
            view.live.snapshot = Some(Arc::new(listing.clone()));
            view.live.surface = Some(Arc::new(fixture_surface(&listing)));
            let mut remote = crate::endpoint::Endpoint::new(
                "ssh:test".into(),
                "Remote".into(),
                ConnectTarget::Ssh {
                    target: "unused".into(),
                    session: "default".into(),
                },
                true,
            );
            remote.live.snapshot = Some(Arc::new(listing));
            remote.live.status = ConnectionStatus::Connected;
            view.endpoints.push(remote);
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.run_until_parked();
    // The resize re-asked for the surface size; the frame follows it, as a
    // real handshake's reply would.
    view.update(cx, |view, _| {
        let surface = Arc::make_mut(view.live.surface.as_mut().unwrap());
        surface.frame.width = view.options.surface_size.cols;
        surface.frame.height = view.options.surface_size.rows;
        let pane = &mut surface.panes[0];
        pane.rect.width = surface.frame.width;
        pane.rect.height = surface.frame.height;
        pane.inner_rect = pane.rect;
        assert!(
            view.input_ready(),
            "fixture window should be ready for input"
        );
    });
    (view, peer, cx)
}

#[gpui::test]
fn the_focused_row_means_a_rescroll_and_other_rows_mean_navigation(cx: &mut gpui::TestAppContext) {
    let (view, _peer, cx) = rescroll_fixture(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.agent_row_action("local", PANE),
            AgentRowAction::Rescroll(RescrollTarget::HerdrPane { pane: PANE.into() })
        );
        assert_eq!(
            view.agent_row_action("ssh:test", PANE),
            AgentRowAction::Navigate
        );
        assert_eq!(
            view.agent_row_action("local", "p1"),
            AgentRowAction::Navigate
        );
    });
}

#[gpui::test]
fn an_unready_or_covered_pane_keeps_navigating(cx: &mut gpui::TestAppContext) {
    let (view, _peer, cx) = rescroll_fixture(cx);
    // Without its surface the window is not ready, so the click keeps
    // the existing queueing.
    let held = view.update(cx, |view, _| {
        let held = view.live.surface.take();
        assert!(!view.input_ready());
        assert_eq!(
            view.agent_row_action("local", PANE),
            AgentRowAction::Navigate
        );
        held
    });
    view.update(cx, |view, cx| {
        assert!(view.open_agent_row("local", PANE, cx));
        assert_eq!(
            view.pending_navigation,
            Some(NavigationTarget::Pane(PANE.into()))
        );
        view.pending_navigation = None;
        view.live.surface = held;
    });
    // A kept herdr-core workspace in front covers the pane, so the click is
    // a real navigation until the user leaves it.
    view.update(cx, |view, cx| {
        let mut session = Session::default();
        session.workspaces.push(CoreWorkspace {
            ahead_behind: None,
            branch: None,
            cwd: "/src".into(),
            custom_label: false,
            id: KEPT.into(),
            keeper: Keeper::App,
            label: KEPT.into(),
            status: CoreAgentStatus::Idle,
            tabs: Vec::new(),
            tokens: Vec::new(),
            worktree: None,
        });
        let scope = crate::browser::scope(&view.endpoints[0]);
        view.set_core_session(scope, session);
        assert!(view.focus_kept_workspace(KEPT, cx));
        assert_eq!(
            view.agent_row_action("local", PANE),
            AgentRowAction::Navigate
        );
        view.leave_kept_workspace();
        assert_eq!(
            view.agent_row_action("local", PANE),
            AgentRowAction::Rescroll(RescrollTarget::HerdrPane { pane: PANE.into() })
        );
    });
}

#[gpui::test]
fn a_second_click_supersedes_a_queued_navigation_and_scrolls(cx: &mut gpui::TestAppContext) {
    let (view, _peer, cx) = rescroll_fixture(cx);
    view.update(cx, |view, cx| {
        view.pending_navigation = Some(NavigationTarget::Tab("w9".into()));
        assert!(view.open_agent_row("local", PANE, cx));
        // A rescroll supersedes what an earlier click queued; the copy mode
        // suite pins the wire frames the dispatch produces.
        assert!(view.pending_navigation.is_none());
    });
}

#[gpui::test]
fn an_unfocused_row_still_navigates(cx: &mut gpui::TestAppContext) {
    let (view, _peer, cx) = rescroll_fixture(cx);
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    let row = cx.debug_bounds("agent-ssh:test-p0").unwrap();
    cx.simulate_click(row.center(), Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.selected_endpoint, 1, "the row's host is selected");
    });
}
