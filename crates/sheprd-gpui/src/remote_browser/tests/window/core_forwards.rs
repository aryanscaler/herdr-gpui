use super::*;

#[gpui::test]
fn a_ready_forward_does_not_open_a_page_for_a_disconnected_core_caller(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = window(cx, &[]);
    view.update(cx, |view, _| {
        let scope = crate::browser::scope(&view.endpoints[0]);
        let mut page = forwarding(
            view.endpoints[0].id.clone(),
            Instant::now() + Duration::from_secs(60),
        );
        page.pane = Some(crate::chat::core_pane::key_on(
            &crate::core_terminals::Host::Remote(scope),
            &sheprd_backend::TerminalId::new("t3").unwrap(),
        ));
        view.remote_browser.push(page);
        view.port_forwards
            .fixture("dev@build", 3000, State::Listening { local_port: 13000 });
    });
    poll(&view, cx);
    assert!(tabs(cx).is_empty());
    view.read_with(cx, |view, _| assert!(view.remote_browser.is_empty()));
}
