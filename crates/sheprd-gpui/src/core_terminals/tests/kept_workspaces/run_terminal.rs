//! Showing a terminal in a kept workspace that already exists, as opening a
//! Herdr 2 orchestrator run does: its own tab, or a new one when none shows it.

use super::*;

#[gpui::test]
fn a_kept_workspace_shows_the_terminal_asked_for(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let first =
                Location::core_terminal(Panes::single(TerminalId::new("t1").unwrap()), false);
            let id = view
                .open_kept_workspace(None, Path::new("/src/run"), first, window, cx)
                .unwrap();
            let tabs = |view: &SheprdWindow, cx: &gpui::App| {
                let (scope, _) = view.browser_key().unwrap();
                cx.global::<Store>().in_workspace(&scope, &id).count()
            };
            assert_eq!(tabs(view, cx), 1);
            // The agent's terminal has no tab there yet: one opens for it.
            let agent = TerminalId::new("t9").unwrap();
            assert!(view.show_terminal_in_kept_workspace(&id, agent.clone(), window, cx));
            assert_eq!(tabs(view, cx), 2);
            assert_eq!(view.kept_in_front(), Some(id.as_str()));
            // Asked again, the same tab is shown rather than another opened.
            assert!(view.show_terminal_in_kept_workspace(&id, agent, window, cx));
            assert_eq!(tabs(view, cx), 2);
        })
    });
}
