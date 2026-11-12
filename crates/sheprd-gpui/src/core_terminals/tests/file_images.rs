use super::{links::shown, worker::until};
use crate::core_terminals::files::{self, Request};
use crate::core_terminals::worker::Command;
use gpui::{ExternalPaths, TestAppContext};
use sheprd_backend::ClientPaneInputEvent;
use sheprd_client::ConnectTarget;
use std::{path::PathBuf, sync::atomic::AtomicBool};

#[gpui::test]
fn preparation_reserves_input_order(cx: &mut TestAppContext) {
    let (view, cx, backend, _) = shown(cx, &["$"], false);
    let answer = cx.update(|_, cx| view.read(cx).pending_paste_for_test());
    let paths = ExternalPaths(vec![PathBuf::from("/after")].into());
    cx.update(|window, cx| view.update(cx, |view, cx| view.drop_files(&paths, window, cx)));
    assert!(
        !backend
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::Input(_)))
    );
    answer
        .send(files::prepare(
            &ConnectTarget::Local,
            Request::Paths(vec![PathBuf::from("/before")]),
            &AtomicBool::new(false),
        ))
        .ok()
        .unwrap();
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
    let pasted = until(|| {
        let values: Vec<_> = backend
            .received
            .lock()
            .unwrap()
            .iter()
            .filter_map(|command| {
                if let Command::Input(ClientPaneInputEvent::Paste(text)) = command {
                    Some(text.clone())
                } else {
                    None
                }
            })
            .collect();
        (values.len() == 2).then_some(values)
    });
    assert_eq!(pasted, ["/before", "/after"]);
}

#[gpui::test]
fn failure_discards_input_held_behind_the_attachment(cx: &mut TestAppContext) {
    let (view, cx, backend, _) = shown(cx, &["$"], false);
    let answer = cx.update(|_, cx| view.read(cx).pending_paste_for_test());
    let paths = ExternalPaths(vec![PathBuf::from("/after")].into());
    cx.update(|window, cx| view.update(cx, |view, cx| view.drop_files(&paths, window, cx)));
    answer
        .send(Err(crate::Error::ConnectionNotReady))
        .ok()
        .unwrap();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.poll(window, cx);
            assert!(matches!(
                view.take_paste_error(),
                Some(crate::Error::ConnectionNotReady)
            ));
        })
    });
    assert!(
        !backend
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::Input(_)))
    );
}

#[gpui::test]
fn host_change_and_disconnect_drop_old_preparation_receivers(cx: &mut TestAppContext) {
    let (view, cx, backend, _) = shown(cx, &["$"], false);
    for disconnect in [false, true] {
        let answer = cx.update(|_, cx| view.read(cx).pending_paste_for_test());
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                if disconnect {
                    view.set_stale(cx);
                } else {
                    view.set_paste_target(Some(ConnectTarget::Ssh {
                        target: "other".into(),
                        session: "same-id".into(),
                    }));
                }
            })
        });
        assert!(answer.send(Err(crate::Error::ConnectionNotReady)).is_err());
    }
    assert!(
        !backend
            .received
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::Input(_)))
    );
}
