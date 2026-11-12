use super::*;
use crate::{hub::Layout, native::changes, web::tests::state_with};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Waker},
    time::Duration,
};

fn state(dir: &Path, path: &Path) -> crate::web::Shared {
    let mut hub = Hub::offline_in(Layout::default(), None, None);
    hub.core = Some(Arc::new(tests_support_source(
        Some(path.into()),
        Some(SCOPE),
    )));
    state_with(dir, Arc::new(hub))
}

fn keeper() -> Keeper {
    Keeper::Saved {
        tab: 4,
        terminal: "t2".into(),
    }
}

fn desktop(path: &Path) -> std::fs::File {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(sheprd_session::saved::lock_path(path))
        .unwrap();
    file.lock().unwrap();
    file
}

#[tokio::test]
async fn absent_and_stale_desktops_edit_saved_layout_without_waiting() {
    for stale in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = saved_file(dir.path());
        let state = state(dir.path(), &path);
        if stale {
            state.changes.seen();
        }
        for op in [
            Op::Zoom,
            Op::RenameTab(Some("phone".into())),
            Op::RenameWorkspace {
                workspace: "w1".into(),
                name: "renamed".into(),
            },
        ] {
            tokio::time::timeout(
                Duration::from_secs(1),
                changes::arrange(&state, keeper(), op),
            )
            .await
            .unwrap()
            .unwrap();
        }
        let saved = read(&path);
        assert_eq!(saved["tabs"][0]["location"]["active"], "t2");
        assert_eq!(saved["tabs"][0]["location"]["zoomed"], "t2");
        assert_eq!(saved["tabs"][0]["location"]["name"], "phone");
        assert_eq!(saved["workspaces"][0]["label"], "renamed");
        assert!(state.changes.take(Duration::ZERO).1.is_empty());
    }
}

#[tokio::test]
async fn exiting_desktop_leaves_an_unclaimed_intent_for_the_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let state = state(dir.path(), &path);
    state.changes.seen();
    let owner = desktop(&path);
    // Poll until the request is queued while the desktop still owns the file.
    let mut change = Box::pin(changes::change_layout(&state, 4, "t2".into(), Op::Zoom));
    assert!(
        change
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(owner);
    tokio::time::timeout(Duration::from_secs(1), change)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read(&path)["tabs"][0]["location"]["zoomed"], "t2");
    assert!(state.changes.take(Duration::ZERO).1.is_empty());
}

#[tokio::test]
async fn claimed_intent_is_not_replayed_after_desktop_exit() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let state = state(dir.path(), &path);
    state.changes.seen();
    let owner = desktop(&path);
    let mut change = Box::pin(changes::change_layout(&state, 4, "t2".into(), Op::Zoom));
    assert!(
        change
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    let (_, asks) = state.changes.take(Duration::ZERO);
    assert_eq!(asks[0].op, Op::Zoom);
    drop(owner);
    // Stand in for the desktop saving the change but losing its answer.
    let source = tests_support_source(Some(path.clone()), Some(SCOPE));
    source.arrange_saved(4, "t2", &Op::Zoom).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), change)
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(read(&path)["tabs"][0]["location"]["zoomed"], "t2");
}

#[test]
fn concurrent_phone_edits_preserve_each_other_and_reopen_under_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let source = tests_support_source(Some(path.clone()), Some(SCOPE));
    std::thread::scope(|threads| {
        threads.spawn(|| source.arrange_saved(4, "t2", &Op::Zoom).unwrap());
        threads.spawn(|| {
            source
                .arrange_saved(4, "t1", &Op::RenamePane(Some("server".into())))
                .unwrap()
        });
        threads.spawn(|| {
            source
                .arrange_saved(4, "t1", &Op::RenameTab(Some("logs".into())))
                .unwrap()
        });
    });
    let _desktop = desktop(&path);
    let saved = sheprd_session::SavedTabs::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.tab_active(SCOPE, 4).unwrap().as_str(), "t2");
    assert_eq!(saved.tab_zoomed(SCOPE, 4).unwrap().as_str(), "t2");
    assert_eq!(saved.tab_name(SCOPE, 4), Some("logs"));
    assert_eq!(saved.pane_names(SCOPE, 4).unwrap()["t1"], "server");
    // Even split cannot start a speculative shell while the app owns it.
    assert!(
        source
            .arrange_saved(4, "t2", &Op::SplitRight)
            .unwrap_err()
            .desktop_owns_layout()
    );
}
