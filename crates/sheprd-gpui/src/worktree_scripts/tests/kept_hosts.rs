use super::*;

#[gpui::test]
fn setup_expires_even_when_its_workspace_exists_on_an_unselected_host(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        let scope = crate::browser::scope(&view.endpoints[0]);
        crate::browser::Store::update(cx, |store| {
            store.keep_workspace(&scope, "lane", "/same/path", false)
        });
        assert!(
            view.kept_workspace_in(&scope, std::path::Path::new("/same/path"), cx)
                .is_some()
        );
        let mut queued = launch(
            view,
            ScriptKind::Setup,
            Some(std::path::Path::new("/same/path")),
        );
        queued.workspace.clear();
        view.queue_kept_setup(queued);
        view.endpoints.push(crate::endpoint::Endpoint::new(
            "ssh:box".into(),
            "Box".into(),
            sheprd_client::ConnectTarget::Ssh {
                target: "box".into(),
                session: "work".into(),
            },
            true,
        ));
        view.selected_endpoint = 1;
        view.poll_kept_scripts(cx);
        assert_eq!(view.queued_kept_setups().len(), 1);
        view.age_kept_setups();
        view.poll_kept_scripts(cx);
        assert!(view.queued_kept_setups().is_empty());
        assert!(view.worktree_script.is_none());
        assert_eq!(
            view.local_error.as_deref(),
            Some(
                crate::i18n::Localize::localize(&crate::worktree_scripts::Error::SetupExpired)
                    .as_str()
            )
        );
    });
}

#[gpui::test]
fn queued_setup_is_cancelled_when_its_saved_route_is_replaced(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        let mut queued = launch(
            view,
            ScriptKind::Setup,
            Some(std::path::Path::new("/same/path")),
        );
        queued.workspace.clear();
        view.queue_kept_setup(queued);
        assert_eq!(view.queued_kept_setups().len(), 1);
        view.endpoints[0].connection.target = sheprd_client::ConnectTarget::Ssh {
            target: "replacement".into(),
            session: "work".into(),
        };
        view.poll_kept_scripts(cx);
        assert!(view.queued_kept_setups().is_empty());
        assert!(view.worktree_script.is_none());
    });
}
