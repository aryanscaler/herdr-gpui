use super::*;

#[gpui::test]
fn remote_conversion_needs_its_own_core_even_when_the_desktop_has_no_acp(cx: &mut TestAppContext) {
    let target = ConnectTarget::Wsl {
        distro: "Ubuntu".into(),
        session: "default".into(),
    };
    let (view, _stream, _program, cx) = attached_on(cx, target);
    view.update(cx, |view, _| {
        view.chats.program = crate::chat::Program::Missing;
        let key = view.core_chat_key(TabId::test(DRIVEN), &id("t1"));
        assert!(matches!(
            view.convert_to_chat(Some(&key)),
            Err(ConvertError::NotConnected)
        ));
    });
}

#[gpui::test]
fn a_core_forwarded_terminal_cannot_launch_a_chat_on_the_connecting_host(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let tab = TabId::test(DRIVEN);
            view.core_terminals.show_for_test(
                tab,
                Panes::single(id("other/t1")),
                false,
                Vec::new(),
            );
            assert!(matches!(
                view.start_core_chat(sheprd_chat_protocol::Agent::Codex, tab, cx),
                Err(StartError::NotLocal)
            ));
        });
    });
}

#[gpui::test]
fn remote_core_prompts_permission_answers_and_cancel_use_the_attached_chat(
    cx: &mut TestAppContext,
) {
    use sheprd_chat_protocol::{Command, PermissionId};
    let target = ConnectTarget::Ssh {
        target: "synthetic-host".into(),
        session: "default".into(),
    };
    let (view, mut stream, _program, cx) = attached_on(cx, target);
    let commands = [
        Command::Prompt {
            text: "remote prompt".into(),
            attachments: Vec::new(),
        },
        Command::Answer {
            option: "allow".into(),
            permission: PermissionId(4),
        },
        Command::Cancel,
    ];
    view.update(cx, |view, _| {
        let key = view.core_chat_key(TabId::test(DRIVEN), &id("t1"));
        for command in &commands {
            view.chats
                .pane_mut(&key)
                .unwrap()
                .send(command.clone())
                .unwrap();
        }
    });
    for expected in commands {
        assert!(
            matches!(read(&mut stream), Some(ClientMessage::Command { command, .. }) if command == expected)
        );
    }
}

#[gpui::test]
fn remote_chat_owns_its_composer_focus_and_split_without_a_local_core(cx: &mut TestAppContext) {
    let target = ConnectTarget::Ssh {
        target: "synthetic-host".into(),
        session: "default".into(),
    };
    let (view, _stream, _program, cx) = attached_on(cx, target);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let tab = TabId::test(DRIVEN);
            let key = view.core_chat_key(tab, &id("t1"));
            assert!(matches!(
                view.chats.pane(&key).unwrap().target.surface,
                Surface::RemoteCore { .. }
            ));
            assert_eq!(
                view.chats.pane(&key).unwrap().target.route,
                Route::Ssh("synthetic-host".into())
            );
            assert!(view.chats.pane("core:t1").is_none());
            assert!(view.core_chat_focus(tab, &id("t1"), window, cx).is_some());
            assert!(
                view.core_chat_focus(TabId::test(MIRROR), &id("t1"), window, cx)
                    .is_none()
            );
            assert_eq!(view.core_chat_tab(&key), Some(tab));
            view.focus_chat(&key, window, cx);
            assert_eq!(view.core_terminals.chat_split_at(tab), Some(id("t1")));
            assert_eq!(view.focused_core_chat_key(window, cx), Some(key));
        });
    });
}

#[gpui::test]
fn removing_a_saved_host_removes_its_chat_instead_of_attaching_locally(cx: &mut TestAppContext) {
    let target = ConnectTarget::Wsl {
        distro: "Ubuntu".into(),
        session: "default".into(),
    };
    let (view, _stream, _program, cx) = attached_on(cx, target);
    view.update(cx, |view, _| {
        let targets = view.core_chat_targets();
        assert_eq!(targets.len(), 1);
        assert!(!targets[0].is_local());
        view.endpoints[view.selected_endpoint].connection.target = ConnectTarget::Local;
        assert!(view.core_chat_targets().is_empty());
    });
}
