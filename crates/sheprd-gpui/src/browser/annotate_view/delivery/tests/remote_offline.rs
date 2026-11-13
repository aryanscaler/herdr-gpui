use super::*;

#[gpui::test]
fn an_offline_core_page_never_uses_a_live_herdr_connection_to_send_local_paths(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.live.snapshot = Some(Arc::new(crate::sidebar::layout_tests::snapshot(1)));
            assert!(
                crate::agent_notes::Origin::of(view).is_some(),
                "Herdr 1 is still connected"
            );
            let scope = scope(&view.endpoints[0]);
            let origin = crate::chat::core_pane::key_on(
                &crate::core_terminals::Host::Remote(scope.clone()),
                &sheprd_backend::TerminalId::new("t3").unwrap(),
            );
            let id = Store::update(cx, |store| {
                store.open(scope.clone(), "w0", None, Some(origin.clone()))
            })
            .unwrap();
            let tab = cx.global::<Store>().get(id).cloned().unwrap();
            let mut note = annotate::Note::new(annotate::Anchor::Page, "Keep for retry").unwrap();
            note.image = Some(Arc::new(Image::from_bytes(ImageFormat::Png, vec![1, 2, 3])));
            view.tab_notes(id).notes.push(note);
            view.send_notes_with(
                &tab,
                |_| panic!("offline remote page saved on the desktop"),
                cx,
            );
            assert!(!view.tab_notes(id).sending.is_pending());
            assert!(view.tab_notes(id).notes[0].sent.is_none());
            assert!(!cx.default_global::<crate::browser::Feedback>().has(
                &crate::browser::FeedbackKey {
                    scope,
                    pane_id: origin,
                }
            ));
        });
    });
    cx.run_until_parked();
}
