//! Herdr 2 agents share the agents list with Herdr 1's: the rows, their
//! order, and the previous, next, and numbered agent steps.

use super::*;
use crate::{
    browser::{Location, Store},
    controls::Command,
    core_terminals::{WORKSPACE, panes::Panes},
    preferences::AgentSort,
    state::ConnectionStatus,
};
use sheprd_backend::TerminalId;
use sheprd_session::{Agent, Keeper, Session, Tab, Workspace};
use std::path::Path;

fn session(workspace: &str, tab: &str) -> Session {
    Session {
        agents: vec![Agent {
            focused: false,
            identity: Some("claude".into()),
            name: "claude".into(),
            pane: "t1".into(),
            status: AgentStatus::Blocked,
            tab: tab.into(),
            workspace: workspace.into(),
        }],
        focused: Some(WORKSPACE.into()),
        workspaces: vec![Workspace {
            ahead_behind: None,
            branch: None,
            cwd: "/src/api".into(),
            custom_label: false,
            id: workspace.into(),
            keeper: Keeper::App,
            label: "api".into(),
            status: AgentStatus::Blocked,
            tabs: vec![Tab {
                focused: false,
                id: tab.into(),
                label: "claude".into(),
                status: AgentStatus::Blocked,
            }],
            tokens: Vec::new(),
            worktree: None,
        }],
    }
}

#[gpui::test]
fn a_herdr_2_agent_is_listed_and_stepped_to_with_herdr_1_agents(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            let mut listing = snapshot(1);
            listing.agents.truncate(1);
            listing.agents[0].agent_status = AgentStatus::Idle;
            view.live.snapshot = Some(Arc::new(listing));
            view.live.status = ConnectionStatus::Connected;
            view.agent_sort = AgentSort::Grouped;
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    let terminal = TerminalId::new("t1").unwrap();
    let location = Location::core_terminal(Panes::single(terminal.clone()), false);
    let workspace = cx
        .update(|window, cx| {
            view.update(cx, |view, cx| {
                view.open_kept_workspace(None, Path::new("/src/api"), location, window, cx)
            })
        })
        .unwrap();
    let tab = view.read_with(cx, |view, cx| {
        let scope = crate::browser::scope(&view.endpoints[0]);
        cx.global::<Store>()
            .in_workspace(&scope, &workspace)
            .next()
            .unwrap()
            .id
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_core_session_for_test(session(&workspace, &u64::from(tab).to_string()));
            // Back to Herdr 1's workspace: the agent's is behind.
            view.leave_kept_workspace();
            cx.notify();
        })
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    // This computer's Herdr 2 agent follows its Herdr 1 one.
    let listed: Vec<String> = cx.update(|_, cx| {
        let view = view.read(cx);
        view.sidebar_agents()
            .into_iter()
            .map(|(_, id)| id.to_owned())
            .collect()
    });
    assert_eq!(listed, ["p0", "t1"]);
    assert!(view.read_with(cx, |view, _| view.kept_in_front().is_none()));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.command(Command::AgentNumber(2), window, cx)
        });
    });
    view.read_with(cx, |view, _| {
        assert_eq!(view.kept_in_front(), Some(workspace.as_str()));
    });
}

#[gpui::test]
fn an_agent_in_a_tab_not_restored_yet_gets_its_own_pane(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture_window(window, cx));
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    let (t1, t2) = (
        TerminalId::new("t1").unwrap(),
        TerminalId::new("t2").unwrap(),
    );
    let mut panes = Panes::single(t1.clone());
    panes
        .split(&t1, crate::core_terminals::panes::Axis::Row, t2.clone())
        .unwrap();
    let location = Location::core_terminal(panes, false);
    let workspace = cx
        .update(|window, cx| {
            view.update(cx, |view, cx| {
                view.open_kept_workspace(None, Path::new("/src/api"), location, window, cx)
            })
        })
        .unwrap();
    let tab = view.read_with(cx, |view, cx| {
        let scope = crate::browser::scope(&view.endpoints[0]);
        cx.global::<Store>()
            .in_workspace(&scope, &workspace)
            .next()
            .unwrap()
            .id
    });
    let endpoint = view.read_with(cx, |view, _| view.endpoints[0].id.clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // As at launch: nothing of the tab restored yet.
            view.forget_core_tabs_for_test();
            view.show_core_agent(
                &endpoint,
                &workspace,
                &u64::from(tab).to_string(),
                "t2",
                window,
                cx,
            );
        })
    });
    cx.update(|_, cx| {
        let shown = view.read(cx).shown_core_pane(cx);
        assert_eq!(shown, Some((tab, t2)));
    });
}
