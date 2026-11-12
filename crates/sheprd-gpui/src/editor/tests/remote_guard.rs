use super::*;

#[gpui::test]
fn a_remote_chat_capable_tab_cannot_receive_a_desktop_editor_path(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, cx| {
        let tab = TabId::test(1);
        let terminal = sheprd_backend::TerminalId::new("t1").unwrap();
        view.core_terminals.show_for_test(
            tab,
            crate::core_terminals::panes::Panes::single(terminal.clone()),
            false,
            Vec::new(),
        );
        view.core_terminals.rehost_for_test(
            tab,
            crate::browser::Scope::endpoint("ssh:box"),
            sheprd_client::ConnectTarget::Ssh {
                target: "box".into(),
                session: "default".into(),
            },
        );
        assert!(view.core_terminals.drives(tab));
        let path = EditorTarget {
            path: PathBuf::from("/desktop/only/file.rs"),
            line: Some(4),
        };
        assert!(matches!(
            view.start_core_editor(&path, tab, terminal, cx),
            Err(crate::Error::Editor(Error::NoPane))
        ));
    });
}
