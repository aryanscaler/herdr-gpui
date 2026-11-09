//! A dropped connection to herdr-core keeps each pane's last frame, dimmed,
//! and sends nothing typed until the terminal is attached again.

use super::{
    accessibility::look,
    worker::{Scripted, size, until},
};
use crate::{
    core_terminals::{
        CoreTerminalView,
        worker::{self, Command},
    },
    terminal_painter::TerminalPainter,
};
use gpui::{EntityInputHandler as _, TestAppContext};
use sheprd_backend::{ClientPaneInputEvent, FrameData, Role, SurfaceUpdate, TerminalId};
use std::{cell::RefCell, rc::Rc};

fn typed(backend: &Scripted) -> Vec<String> {
    backend
        .received
        .lock()
        .unwrap()
        .iter()
        .filter_map(|command| match command {
            Command::Input(ClientPaneInputEvent::TextCommit(text)) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn frame() -> FrameData {
    FrameData {
        cells: Vec::new(),
        width: 0,
        height: 0,
        cursor: None,
        hyperlinks: Vec::new(),
        graphics: Vec::new(),
    }
}

#[gpui::test]
fn typing_while_stale_is_dropped_and_told_then_sent_once_attached_again(cx: &mut TestAppContext) {
    let first = Scripted::default();
    first.push(vec![SurfaceUpdate::Frame(frame())]);
    let terminal = TerminalId::new("t1").unwrap();
    let link = worker::spawn(first.clone(), terminal.clone(), size(80, 24), Role::Active).unwrap();
    let painter = Rc::new(RefCell::new(TerminalPainter::default()));
    let (view, cx) = cx.add_window_view(|_, cx| {
        CoreTerminalView::new(terminal.clone(), link, look(), painter, false, cx)
    });
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });

    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_stale(cx);
            assert!(view.is_stale());
            view.replace_text_in_range(None, "ls", window, cx);
            assert!(view.take_dropped(), "the window is told");
            assert!(!view.take_dropped(), "once");
        })
    });

    let second = Scripted::default();
    let link = worker::spawn(second.clone(), terminal, size(80, 24), Role::Active).unwrap();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.relink(link, cx);
            assert!(!view.is_stale());
            view.replace_text_in_range(None, "pwd", window, cx);
            assert!(!view.take_dropped());
        })
    });
    until(|| (!typed(&second).is_empty()).then_some(()));
    assert_eq!(typed(&second), ["pwd"]);
    assert!(typed(&first).is_empty(), "nothing typed while stale");
}

#[gpui::test]
fn the_last_frame_stays_until_the_terminal_draws_again(cx: &mut TestAppContext) {
    let first = Scripted::default();
    first.push(vec![SurfaceUpdate::Frame(frame())]);
    let terminal = TerminalId::new("t1").unwrap();
    let link = worker::spawn(first, terminal.clone(), size(80, 24), Role::Active).unwrap();
    let painter = Rc::new(RefCell::new(TerminalPainter::default()));
    let (view, cx) = cx.add_window_view(|_, cx| {
        CoreTerminalView::new(terminal.clone(), link, look(), painter, false, cx)
    });
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    // Attached again, the terminal reports its title before a frame.
    let second = Scripted::default();
    second.push(vec![SurfaceUpdate::Title("sh".into())]);
    let link = worker::spawn(second, terminal, size(80, 24), Role::Active).unwrap();
    cx.update(|_, cx| view.update(cx, |view, cx| view.relink(link, cx)));
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.title(), Some("sh"));
        assert!(view.has_frame(), "the old frame is kept");
    });
}
