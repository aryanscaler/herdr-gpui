use super::*;

#[gpui::test]
fn an_explicit_unknown_core_pane_never_falls_back_to_herdr_ones_workspace(
    cx: &mut gpui::TestAppContext,
) {
    let (_view, cx) = cx.add_window_view(fixture_window);
    cx.cx.update(|cx| {
        for pane in ["core:t-missing", "core@7:ssh:box:t-missing"] {
            let response = serve(
                call(ChatOp::New {
                    agent: "codex".into(),
                    pane: Some(pane.into()),
                }),
                cx,
            );
            assert_eq!(code(&response), Some(ErrorCode::NotFound));
        }
    });
}
