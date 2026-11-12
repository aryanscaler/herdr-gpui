use super::*;

#[gpui::test]
fn a_removed_checkout_drops_its_tabs_without_queuing_another_kill(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, true);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t-ended").unwrap()), false);
            let path = Path::new("/src/ended-lane");
            let id = view
                .open_kept_workspace_behind("ended", path, location, cx)
                .unwrap();
            let scope = view.local_kept_scope().unwrap();
            let before = view.core_terminals.unended.lock().unwrap().clone();
            view.forget_removed_kept_workspace_in(&scope, &id, window, cx);
            assert!(view.kept_workspace_in(&scope, path, cx).is_none());
            assert_eq!(*view.core_terminals.unended.lock().unwrap(), before);
        })
    });
}
