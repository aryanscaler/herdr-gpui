use super::*;
use crate::worktree_scripts::{Config, Job};
use std::sync::Arc;

#[gpui::test]
fn a_trusted_read_cannot_start_after_the_selection_changes(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let config = Config::parse(b"[scripts]\nrun = 'make dev'\n").unwrap();
            let mut launch = launch(view, ScriptKind::Run, Some(std::path::Path::new("/repo")));
            launch.workspace = "space-1".into();
            cx.default_global::<Trust>().grant(launch.grant(&config));
            view.worktree_script = Some(Job {
                launch,
                step: Step::Reading,
                cancel: Arc::new(AtomicBool::new(false)),
            });
            view.selection_epoch += 1;
            assert!(matches!(
                view.open_worktree_script_tab(&config, cx),
                Err(crate::Error::StaleConnection)
            ));
            view.worktree_scripts_read(Ok(Some(config)), cx);
            assert!(view.worktree_script.is_none());
            assert!(view.local_error.is_some());
        })
    });
}

#[gpui::test]
fn a_script_cannot_start_in_a_workspace_that_cannot_be_focused(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(matches!(
                view.start_in_kept_workspace("space-missing", sheprd_backend::Spawn::default(), cx),
                Err(crate::Error::StaleWorkspace)
            ));
        })
    });
}
