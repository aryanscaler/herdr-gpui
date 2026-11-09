//! Moving a Herdr 2 agent into a chat: what the window refuses before any
//! worker starts.

use crate::{
    browser::TabId, chat::ConvertError, core_terminals::panes::Panes,
    sidebar::layout_tests::fixture_window,
};
use sheprd_backend::{AgentStatus, Process, RunState, TerminalId, TerminalInfo};
use sheprd_client::herdr::convert;

fn claude(status: AgentStatus) -> TerminalInfo {
    TerminalInfo {
        agent: status,
        cwd: Some("/work".into()),
        foreground: Some(Process {
            argv: vec!["claude".into()],
            name: "claude".into(),
            pid: 5,
        }),
        id: TerminalId::new("t1").unwrap(),
        labels: Default::default(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: None,
    }
}

#[gpui::test]
fn a_core_pane_is_asked_of_the_core_and_waits_for_it(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.config.features.acp_chats = true;
            view.core_terminals.show_for_test(
                TabId::test(3),
                Panes::single(TerminalId::new("t1").unwrap()),
                false,
                vec![claude(AgentStatus::Idle)],
            );
            // The key names a herdr-core terminal, so Herdr 1 is not asked;
            // with no core connected the move does not start.
            assert!(matches!(
                view.convert_to_chat(Some("core:t1")),
                Err(ConvertError::NotConnected | ConvertError::NotReachable)
            ));
            assert!(view.chats.converting_core.is_none());
            assert!(view.chats.converting.is_none());
        })
    });
}

#[gpui::test]
fn a_busy_core_agent_is_refused_by_the_shared_policy(cx: &mut gpui::TestAppContext) {
    let info = claude(AgentStatus::Working);
    assert!(matches!(
        sheprd_core_client::convert::check(&info),
        Err(convert::Error::Busy(_))
    ));
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.config.features.acp_chats = false;
            assert!(matches!(
                view.convert_to_chat(Some("core:t1")),
                Err(ConvertError::Disabled)
            ));
        })
    });
}

#[test]
fn a_focused_pane_on_another_hosts_core_is_refused() {
    use crate::chat::convert::focused_core_key;
    let t1 = TerminalId::new("t1").unwrap();
    let (local, remote) = (TabId::test(1), TabId::test(2));
    let drives = |tab: TabId| tab == local;
    assert_eq!(
        focused_core_key(Some((local, t1.clone())), drives)
            .unwrap()
            .as_deref(),
        Some("core:t1")
    );
    // Not Herdr's focused pane instead, and not this computer's `t1`.
    assert!(matches!(
        focused_core_key(Some((remote, t1)), drives),
        Err(ConvertError::NotReachable)
    ));
    assert_eq!(focused_core_key(None, drives).unwrap(), None);
}
