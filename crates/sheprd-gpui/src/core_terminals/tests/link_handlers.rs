use super::links::{at, frame, link_modifier, shown};
use super::worker::until;
use crate::core_terminals::view::ToWindow;
use gpui::{Modifiers, MouseButton, TestAppContext};
use sheprd_backend::SurfaceUpdate;

#[gpui::test]
fn remote_file_hyperlink_keeps_raw_target_and_only_emits_on_click(cx: &mut TestAppContext) {
    assert_explicit_click(cx, "file:///remote/notes%20one.md", true);
}

#[gpui::test]
fn missing_local_file_hyperlink_reaches_handlers_in_mouse_reporting_panes(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing notes.md");
    let url = url::Url::from_file_path(&missing).unwrap();
    assert!(!missing.exists());
    assert_explicit_click(cx, url.as_str(), false);
    assert!(!missing.exists());
}

fn assert_explicit_click(cx: &mut TestAppContext, raw: &str, remote: bool) {
    let (view, cx, backend, asked) = shown(cx, &["notes"], false);
    let mut screen = frame(40, &["notes"]);
    screen.hyperlinks.push(raw.into());
    for cell in &mut screen.cells[..5] {
        cell.hyperlink = Some(0);
    }
    backend.push(vec![
        SurfaceUpdate::Frame(screen),
        SurfaceUpdate::MouseReporting(true),
    ]);
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_remote(remote, cx)));
    let point = at(&view, cx, 2, 0);
    cx.simulate_mouse_move(point, None, link_modifier());
    cx.run_until_parked();
    if !remote {
        cx.update(|_, cx| {
            assert!(
                view.read(cx).link_rows().is_empty(),
                "missing file remains unverified"
            )
        });
    }
    assert!(
        asked.borrow().is_empty(),
        "hover must not invoke an adapter"
    );
    cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::none());
    assert!(asked.borrow().is_empty());
    cx.simulate_mouse_down(point, MouseButton::Left, link_modifier());
    cx.simulate_mouse_up(point, MouseButton::Left, link_modifier());
    let asks = asked.borrow();
    let [ToWindow::OpenLink(click)] = asks.as_slice() else {
        panic!("one click expected: {asks:?}")
    };
    assert_eq!(click.handler_url.as_deref(), Some(raw));
}
