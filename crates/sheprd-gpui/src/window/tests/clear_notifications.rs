use super::*;
use crate::{
    notifications::{Notice, tests::notification},
    window::{main_window::MainWindow, system_notifications},
};
use gpui::{
    AppContext, Context, Entity, IntoElement, Render, TestAppContext, Window, WindowHandle,
};
use sheprd_backend::TerminalId;
use sheprd_client::protocol::SemanticNotificationKind as Kind;
use std::time::Instant;

/// Gives `handle`'s host an alert that goes to the OS notification center,
/// and posts a Herdr 2 agent's notification about `terminal` from it.
fn post(handle: WindowHandle<SheprdWindow>, terminal: &str, cx: &mut TestAppContext) {
    handle
        .update(cx, |view, window, cx| post_in(view, terminal, window, cx))
        .unwrap();
}

fn post_in(
    view: &mut SheprdWindow,
    terminal: &str,
    window: &mut Window,
    cx: &mut Context<SheprdWindow>,
) {
    view.config.notifications.enabled = false;
    view.config.notifications.system = true;
    view.config.notifications.delay_seconds = 0;
    let snapshot = Arc::new(snapshot(2));
    view.live.snapshot = Some(snapshot.clone());
    view.endpoints[0].live.snapshot = Some(snapshot.clone());
    let mut wire = notification("claude needs you");
    wire.kind = Kind::NeedsAttention;
    wire.workspace_id = Some("w0".into());
    wire.tab_id = Some("t1".into());
    wire.pane_id = None;
    let now = Instant::now();
    let notice = Notice::new(wire, now).with_snapshot(Some(&snapshot));
    view.endpoints[0].toasts.receive([notice]);
    view.tick_toasts(false, now, cx);
    view.post_system_notifications(window, cx);
    let terminal = TerminalId::new(terminal).unwrap();
    let main = MainWindow::of(window, cx).unwrap();
    view.post_core_notification(main, &terminal, Kind::Finished, cx);
}

fn kept(handle: WindowHandle<SheprdWindow>, cx: &mut TestAppContext) -> (usize, usize) {
    handle
        .read_with(cx, |view, _| {
            let toasts = &view.endpoints[0].toasts;
            (toasts.entries.len(), toasts.history.alerts().count())
        })
        .unwrap()
}

/// One command, in either window, clears both windows and takes back every
/// OS notification the app posted, Herdr 1 and Herdr 2 alike.
#[gpui::test]
fn clear_all_notifications_reaches_every_window_and_the_os(cx: &mut TestAppContext) {
    let first = cx.add_window(fixture_window);
    let second = cx.add_window(fixture_window);
    cx.update(system_notifications::install_registry);
    post(first, "t-1", cx);
    post(second, "t-2", cx);
    cx.run_until_parked();
    let mut posted: Vec<String> = cx
        .shown_system_notifications()
        .into_iter()
        .map(|shown| shown.tag.to_string())
        .collect();
    posted.sort();
    assert_eq!(posted.len(), 4, "{posted:?}");
    assert!(posted.iter().any(|tag| tag == "herdr-core:t-1"));
    // Each window kept its alert and the posted notice its click answers.
    assert_eq!(kept(first, cx), (1, 1));
    assert_eq!(kept(second, cx), (1, 1));

    second
        .update(cx, |view, window, cx| {
            view.command(Command::ClearAllNotifications, window, cx)
        })
        .unwrap();
    cx.run_until_parked();

    assert_eq!(kept(first, cx), (0, 0));
    assert_eq!(kept(second, cx), (0, 0));
    let mut dismissed: Vec<String> = cx
        .dismissed_system_notifications()
        .into_iter()
        .map(|tag| tag.to_string())
        .collect();
    dismissed.sort();
    assert_eq!(dismissed, posted);
    // Their routes are gone, so a late click on one opens nothing.
    cx.update(|cx| {
        for tag in &posted {
            assert!(!system_notifications::open(tag, cx), "{tag}");
        }
    });
    // A second clear has nothing left to take back.
    first
        .update(cx, |view, window, cx| {
            view.command(Command::ClearAllNotifications, window, cx)
        })
        .unwrap();
    assert_eq!(cx.dismissed_system_notifications().len(), posted.len());
}

/// Banners whose click routes the registry already evicted are withdrawn too.
#[gpui::test]
fn banners_past_the_click_routes_are_withdrawn(cx: &mut TestAppContext) {
    let window = cx.add_window(fixture_window);
    cx.update(system_notifications::install_registry);
    let count = 70;
    window
        .update(cx, |view, window, cx| {
            view.config.notifications.enabled = false;
            view.config.notifications.system = true;
            let main = MainWindow::of(window, cx).unwrap();
            for index in 0..count {
                let terminal = TerminalId::new(format!("t-{index}")).unwrap();
                view.post_core_notification(main.clone(), &terminal, Kind::Finished, cx);
            }
            view.command(Command::ClearAllNotifications, window, cx);
        })
        .unwrap();
    let dismissed = cx.dismissed_system_notifications();
    assert_eq!(dismissed.len(), count);
    assert!(dismissed.iter().any(|tag| tag == "herdr-core:t-0"));
}

/// A host's root view holding a main view, as Moonframe's does.
struct Host {
    main: Entity<SheprdWindow>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.main.clone()
    }
}

/// A main view under a host's root clears with the app's own windows, and
/// runs the command for all of them.
#[gpui::test]
fn a_hosted_main_view_clears_with_the_others(cx: &mut TestAppContext) {
    let root = cx.add_window(fixture_window);
    let host = cx.add_window(|window, cx| Host {
        main: cx.new(|cx| {
            MainWindow::register(window, cx);
            fixture_window(window, cx)
        }),
    });
    cx.update(system_notifications::install_registry);
    post(root, "t-1", cx);
    let hosted = host.update(cx, |host, _, _| host.main.clone()).unwrap();
    host.update(cx, |_, window, cx| {
        hosted.update(cx, |view, cx| post_in(view, "t-2", window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    let shown = cx.shown_system_notifications().len();
    assert_eq!(shown, 4);
    host.update(cx, |_, window, cx| {
        hosted.update(cx, |view, cx| {
            view.command(Command::ClearAllNotifications, window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kept(root, cx), (0, 0));
    let hosted_kept = cx.update(|cx| {
        let toasts = &hosted.read(cx).endpoints[0].toasts;
        (toasts.entries.len(), toasts.history.alerts().count())
    });
    assert_eq!(hosted_kept, (0, 0));
    assert_eq!(cx.dismissed_system_notifications().len(), shown);
}
