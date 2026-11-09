//! Herdr 2 agents on the Overview, beside this computer's Herdr 1 agents.

use super::page::window;
use crate::{
    browser::TabId,
    core_terminals::panes::Panes,
    overview::{Open, State},
    sidebar::ActivityPanel,
};
use sheprd_backend::{AgentStatus, Process, RunState, TerminalId, TerminalInfo};

fn info(id: &str, agent: AgentStatus, program: &str) -> TerminalInfo {
    TerminalInfo {
        agent,
        cwd: Some("/work/herdr".into()),
        foreground: Some(Process {
            argv: vec![program.to_owned()],
            name: program.to_owned(),
            pid: 4,
        }),
        id: TerminalId::new(id).unwrap(),
        labels: Default::default(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: Some("Fix the tests".into()),
    }
}

#[gpui::test]
fn core_agents_are_listed_and_open_their_pane(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let tab = TabId::test(7);
    let mut panes = Panes::single(TerminalId::new("t1").unwrap());
    panes
        .split(
            &TerminalId::new("t1").unwrap(),
            crate::core_terminals::panes::Axis::Row,
            TerminalId::new("t2").unwrap(),
        )
        .unwrap();
    let rows = cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.core_terminals.show_for_test(
                tab,
                panes,
                false,
                vec![
                    info("t1", AgentStatus::Blocked, "claude"),
                    info("t2", AgentStatus::Unknown, "zsh"),
                ],
            );
            view.poll_overview(cx);
            view.overview_rows(cx)
        })
    });
    let core: Vec<_> = rows
        .iter()
        .filter(|row| matches!(row.open, Open::Core { .. }))
        .collect();
    assert_eq!(core.len(), 1, "the shell is no agent");
    let row = core[0];
    assert_eq!(row.state, State::Blocked);
    assert_eq!(row.identity.as_deref(), Some("claude"));
    assert_eq!(
        (row.title.as_str(), row.workspace.as_str()),
        ("Fix the tests", "herdr")
    );
    assert!(row.since.is_some(), "the poll noted when it was blocked");
    // Listed with this computer's Herdr 1 agents, after them.
    let index = rows.iter().position(|listed| listed == row).unwrap();
    assert_eq!(rows[index - 1].endpoint, row.endpoint);
    let row = row.clone();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_overview_row(&row, window, cx);
            assert_eq!(view.activity_section(), None, "back among the terminals");
            assert_eq!(view.activity.return_to, Some(ActivityPanel::Overview));
        })
    });
}

#[gpui::test]
fn a_core_agent_takes_a_reply_that_waits_for_the_core(cx: &mut gpui::TestAppContext) {
    use crate::overview::replies::Presence;
    let (view, cx) = window(cx);
    let tab = TabId::test(7);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.core_terminals.show_for_test(
                tab,
                Panes::single(TerminalId::new("t1").unwrap()),
                false,
                vec![info("t1", AgentStatus::Idle, "claude")],
            );
            let row = view
                .overview_rows(cx)
                .into_iter()
                .find(|row| matches!(row.open, Open::Core { .. }))
                .unwrap();
            assert_eq!(
                row.reply_pane(),
                Some((crate::overview::core::ENDPOINT, "t1"))
            );
            let target = view.overview_reply_target(&row).unwrap();
            assert_eq!(
                (target.pane.as_str(), target.agent.as_str()),
                ("t1", "claude")
            );
            // No core is connected in the test: nothing is known, so a reply
            // waits rather than being dropped.
            assert_eq!(
                view.reply_presence((crate::overview::core::ENDPOINT, "t1"), "", "claude"),
                Presence::Unreachable
            );
        })
    });
}
