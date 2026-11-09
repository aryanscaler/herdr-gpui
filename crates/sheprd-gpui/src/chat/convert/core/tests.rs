use super::terminal;

#[test]
fn a_core_chat_key_names_its_terminal() {
    assert_eq!(terminal("core:t7").unwrap().as_str(), "t7");
    assert_eq!(terminal("core:box/t7").unwrap().as_str(), "box/t7");
    assert!(terminal("p_1").is_none());
    assert!(terminal("core:").is_none());
}

#[gpui::test]
fn turning_chats_off_stops_a_core_move_underway(cx: &mut gpui::TestAppContext) {
    use std::sync::atomic::Ordering;
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.config.features.acp_chats = true;
            let (job, cancelled) = super::Job::waiting_for_test();
            view.chats.converting_core = Some(job);
            view.config.features.acp_chats = false;
            assert!(matches!(
                view.poll_core_convert(),
                Some(Err(super::Error::Disabled))
            ));
            assert!(view.chats.converting_core.is_none());
            assert!(cancelled.load(Ordering::Acquire), "the worker is told");
        })
    });
}
