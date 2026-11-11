use super::*;
use gpui::{Entity, IntoElement, Render, TestAppContext};

/// A host's root view, holding a main view as Moonframe does.
struct Host {
    main: Entity<SheprdWindow>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.main.clone()
    }
}

#[gpui::test]
fn a_main_view_under_a_hosts_root_is_found_and_updated(cx: &mut TestAppContext) {
    let host = cx.add_window(|window, cx| Host {
        main: cx.new(|cx| {
            MainWindow::register(window, cx);
            crate::sidebar::layout_tests::fixture_window(window, cx)
        }),
    });
    let mains = cx.update(|cx| MainWindow::all(cx));
    assert_eq!(mains.len(), 1);
    assert!(matches!(mains[0], MainWindow::Hosted { .. }));
    assert_eq!(mains[0].window_id(), host.window_id());
    mains[0]
        .update(cx, |view, _, _| view.local_error = Some("seen".into()))
        .unwrap();
    let error = cx.update(|cx| mains[0].read(cx).unwrap().local_error.clone());
    assert_eq!(error.as_deref(), Some("seen"));
    // Once the host's window closes, it is no main window.
    cx.update(|cx| {
        host.update(cx, |_, window, _| window.remove_window())
            .unwrap()
    });
    cx.run_until_parked();
    assert!(cx.update(|cx| MainWindow::all(cx)).is_empty());
}

#[gpui::test]
fn a_window_whose_root_is_the_main_view_needs_no_registering(cx: &mut TestAppContext) {
    let window = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    let mains = cx.update(|cx| MainWindow::all(cx));
    assert_eq!(mains, [MainWindow::Root(window)]);
}
