//! Herdr's shared terminal settings on a herdr-core pane: `copy_on_select`,
//! `mouse_scroll_lines`, and `right_click_passthrough_modifier`, with the
//! GUI's `keep_selection_after_copy`.

use super::{
    accessibility::look,
    links::{Asked, at, shown},
    worker::{Scripted, until},
};
use crate::{
    core_terminals::{CoreTerminalView, Look, view::ToWindow, worker::Command},
    terminal::PLATFORM_NOTCH_LINES,
};
use gpui::{
    Entity, Modifiers, MouseButton, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext,
    point,
};
use sheprd_backend::{
    ClientMouseButton, ClientMouseKind, ClientPaneInputEvent, HistoryAnswer, SurfaceUpdate,
};
use std::num::NonZeroU16;

fn set_look(view: &Entity<CoreTerminalView>, cx: &mut VisualTestContext, look: Look) {
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_look(look, cx)));
}

fn drag(view: &Entity<CoreTerminalView>, cx: &mut VisualTestContext, from: u16, to: u16) {
    let (start, end) = (at(view, cx, from, 0), at(view, cx, to, 0));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

/// Has the terminal answer the next history read with `text`, as a copy
/// reads the selection from the history.
fn answers(backend: &Scripted, text: &str) {
    backend
        .answers
        .lock()
        .unwrap()
        .push(HistoryAnswer::Text(text.into()));
}

/// The clipboard once the copy's read is answered and taken.
fn landed(view: &Entity<CoreTerminalView>, cx: &mut VisualTestContext) -> Option<String> {
    until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.poll(window, cx));
            (!view.read(cx).copy_pending()).then_some(())
        })
    });
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
    clipboard(cx)
}

fn copied(asked: &Asked) -> usize {
    asked
        .borrow()
        .iter()
        .filter(|ask| matches!(ask, ToWindow::Copied))
        .count()
}

/// The mouse reports the program received.
fn reports(backend: &Scripted) -> Vec<ClientPaneInputEvent> {
    backend
        .received
        .lock()
        .unwrap()
        .iter()
        .filter_map(|command| match command {
            Command::Input(input @ ClientPaneInputEvent::Mouse { .. }) => Some(input.clone()),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn releasing_a_selection_copies_it_and_says_so(cx: &mut TestAppContext) {
    let (view, cx, backend, asked) = shown(cx, &["hello world"], false);
    set_look(
        &view,
        cx,
        Look {
            keep_selection: false,
            ..look()
        },
    );
    answers(&backend, "hello");
    drag(&view, cx, 0, 4);
    assert_eq!(landed(&view, cx).as_deref(), Some("hello"));
    assert_eq!(copied(&asked), 1);
    // Without `keep_selection_after_copy` the highlight goes.
    cx.update(|_, cx| assert!(view.read(cx).highlights().is_empty()));
}

#[gpui::test]
fn a_kept_selection_stays_highlighted_after_the_copy(cx: &mut TestAppContext) {
    let (view, cx, backend, asked) = shown(cx, &["hello world"], false);
    answers(&backend, "world");
    drag(&view, cx, 6, 10);
    assert_eq!(landed(&view, cx).as_deref(), Some("world"));
    assert_eq!(copied(&asked), 1);
    cx.update(|_, cx| assert!(!view.read(cx).highlights().is_empty()));
}

#[gpui::test]
fn without_copy_on_select_the_selection_waits_for_a_copy_key(cx: &mut TestAppContext) {
    let (view, cx, backend, asked) = shown(cx, &["hello world"], false);
    set_look(
        &view,
        cx,
        Look {
            copy_on_select: false,
            ..look()
        },
    );
    drag(&view, cx, 0, 4);
    assert_eq!(clipboard(cx), None);
    assert_eq!(copied(&asked), 0);
    cx.update(|window, cx| {
        let focus = view.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    });
    // Ctrl-C copies too while the highlight waits, as in Herdr.
    let key = if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    };
    answers(&backend, "hello");
    cx.simulate_keystrokes(key);
    assert_eq!(landed(&view, cx).as_deref(), Some("hello"));
    assert_eq!(copied(&asked), 1);
}

#[gpui::test]
fn a_copy_that_reads_no_text_says_nothing_and_keeps_the_selection(cx: &mut TestAppContext) {
    let (view, cx, _, asked) = shown(cx, &["hello world"], false);
    set_look(
        &view,
        cx,
        Look {
            keep_selection: false,
            ..look()
        },
    );
    // The history answers the read with no text, so nothing is copied.
    drag(&view, cx, 0, 4);
    assert_eq!(landed(&view, cx), None);
    assert_eq!(copied(&asked), 0, "a copy is said only once it lands");
    cx.update(|_, cx| assert!(!view.read(cx).highlights().is_empty()));
}

#[gpui::test]
fn a_wheel_notch_scrolls_the_configured_lines(cx: &mut TestAppContext) {
    let (view, cx, backend, _) = shown(cx, &["$"], false);
    set_look(
        &view,
        cx,
        Look {
            scroll_lines: NonZeroU16::new(5).unwrap(),
            ..look()
        },
    );
    let position = at(&view, cx, 0, 0);
    cx.simulate_event(ScrollEvent {
        position,
        delta: ScrollDelta::Lines(point(0., f32::from(PLATFORM_NOTCH_LINES))),
        ..Default::default()
    });
    let lines = until(|| {
        reports(&backend).into_iter().find_map(|input| match input {
            ClientPaneInputEvent::Mouse {
                kind: ClientMouseKind::ScrollUp,
                lines,
                ..
            } => Some(lines),
            _ => None,
        })
    });
    assert_eq!(lines, 5);
}

#[gpui::test]
fn the_passthrough_modifier_reaches_the_program_without_it(cx: &mut TestAppContext) {
    let (view, cx, backend, _) = shown(cx, &["$"], false);
    backend.push(vec![SurfaceUpdate::MouseReporting(true)]);
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    set_look(
        &view,
        cx,
        Look {
            right_click_passthrough: crate::herdr_settings::RightClickPassthrough::held(
                Modifiers::alt(),
            ),
            ..look()
        },
    );
    let position = at(&view, cx, 0, 0);
    let next = at(&view, cx, 2, 0);
    // The whole gesture, drag included, goes without the modifier.
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::alt());
    cx.simulate_mouse_move(next, Some(MouseButton::Right), Modifiers::alt());
    cx.simulate_mouse_up(next, MouseButton::Right, Modifiers::alt());
    // Another modifier is reported as held.
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::control());
    let modifiers: Vec<(ClientMouseKind, u8)> = until(|| {
        let reports = reports(&backend);
        (reports.len() >= 4).then_some(reports)
    })
    .into_iter()
    .filter_map(|input| match input {
        ClientPaneInputEvent::Mouse {
            kind, modifiers, ..
        } => Some((kind, modifiers)),
        _ => None,
    })
    .collect();
    assert_eq!(
        modifiers,
        [
            (ClientMouseKind::Down(ClientMouseButton::Right), 0),
            (ClientMouseKind::Drag(ClientMouseButton::Right), 0),
            (ClientMouseKind::Up(ClientMouseButton::Right), 0),
            (ClientMouseKind::Down(ClientMouseButton::Right), 0b10),
        ]
    );
}

#[gpui::test]
fn a_copy_landing_late_leaves_a_selection_begun_since(cx: &mut TestAppContext) {
    let (view, cx, backend, asked) = shown(cx, &["hello world"], false);
    set_look(
        &view,
        cx,
        Look {
            keep_selection: false,
            ..look()
        },
    );
    answers(&backend, "hello");
    drag(&view, cx, 0, 4);
    // Another drag begins before the first copy's text lands.
    let (start, end) = (at(&view, cx, 6, 0), at(&view, cx, 10, 0));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    assert_eq!(landed(&view, cx).as_deref(), Some("hello"));
    assert_eq!(copied(&asked), 1);
    // The copy drops only its own selection: the new one stays.
    cx.update(|_, cx| assert!(!view.read(cx).highlights().is_empty()));
}
