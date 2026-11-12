//! `sheprd orchestrator --repo` names one of several loaded projects. A run
//! it restarts or adopts is that project's, whichever one was active.
use super::*;
use crate::orchestrator::{
    Request, Timing,
    view::{RepoSpec, dispatch::Confirm, repo::Repo},
};
use std::sync::Arc;

#[gpui::test]
fn a_named_project_is_where_its_run_is_restarted(cx: &mut gpui::TestAppContext) {
    let (dir, view, cx) = empty::open(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let issue = item(Provider::Github, "7", "Icons", "2026-10-04T00:00:00Z");
            let found = run("r7", &issue, RunState::Running, "2026-10-04T01:00:00Z");
            let session = HerdrSession {
                pane: Pane::Herdr2 {
                    terminal: "t9".into(),
                },
                ..session(&found, "")
            };
            view.repos.push(Repo::start(RepoSpec {
                key: "other".into(),
                host: "9950x3d.local".into(),
                request: Request {
                    target: sheprd_client::ConnectTarget::Local,
                    checkout: dir.path().join("other").to_string_lossy().into_owned(),
                    workspace_id: "other".into(),
                    token: None,
                    data_root: Some(dir.path().join("other-data")),
                    azure_store: Default::default(),
                    gitlab_store: Default::default(),
                    timing: Timing::default(),
                },
            }));
            let other = &mut view.repos[1];
            other.snapshot.items = Arc::new(vec![issue]);
            // Listing rebuilds the shown runs from the recorded ones.
            other.recorded_runs = Arc::new(vec![found]);
            other.snapshot.sessions = Arc::new(vec![session]);
            view.refresh_rows();
            view.active = 0;
            assert!(
                !view.confirm_restart("r7", cx),
                "the first project has no such run"
            );

            // The first project's confirmation is still open when the
            // command arrives: it must not survive into the other project.
            view.confirm = Some(Confirm::Restart { run: "r1".into() });
            view.only_project("other".into(), cx);
            assert_eq!(view.active, 1);
            assert!(view.confirm.is_none());
            assert!(view.confirm_restart("r7", cx));
            assert!(matches!(
                &view.confirm,
                Some(Confirm::Restart { run }) if run == "r7"
            ));
        })
    });
}
