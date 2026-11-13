use super::*;
use crate::pull_request::{Cache, Origin, View};
use std::sync::Arc;

#[test]
fn ssh_kept_badge_and_counts_read_the_prefetch_input_without_h1_lookup() {
    let mut workspace = crate::session_model::of_snapshot(&layout_tests::snapshot(6))
        .workspaces
        .remove(4);
    workspace.keeper = sheprd_session::Keeper::App;
    workspace.cwd = "/remote/checkout".into();
    workspace.worktree.as_mut().unwrap().key = "/remote/repo/.git".into();
    let input = crate::pull_request::kept_input(&workspace, true).unwrap();
    let view = View {
        origin: Origin::Ssh("box".into()),
        login: "remote-account".into(),
    };
    let mut cache = Cache::default();
    cache.scope(
        (0, 0, String::new()),
        Arc::new("fixture".into()),
        &view.login,
        view.origin.clone(),
    );
    let mut pr = crate::pull_request::fixture().unwrap();
    pr.additions = 1730;
    pr.deletions = 31;
    cache.seed(input.clone(), pr, Instant::now());
    assert!(
        cache
            .peek(&view, input.repo_key.as_deref().unwrap(), &input.branch)
            .is_none(),
        "no secondary H1 lookup is seeded"
    );
    let marks = crate::teleport::Marks::detached();
    let theme = Theme::default();
    // A macro rather than a closure: each call borrows its own view and
    // workspace, which a closure's single inferred lifetime cannot express.
    macro_rules! badge {
        ($workspace:expr, $view:expr) => {
            workspace_badge(
                $workspace,
                Some((&cache, $view)),
                (None, ChangeCounts::PullRequest),
                (&marks, "ssh:box"),
                (None, false),
                &theme,
            )
        };
    }
    let found = badge!(&workspace, &view).unwrap();
    assert_eq!(found.pr.unwrap().number, "#8");
    let counts = found.counts.unwrap();
    assert_eq!(
        (counts.additions.as_str(), counts.deletions.as_str()),
        ("+1730", "-31")
    );
    for wrong in [
        View {
            origin: Origin::Local,
            ..view.clone()
        },
        View {
            origin: Origin::Ssh("other".into()),
            ..view.clone()
        },
        View {
            login: "other-account".into(),
            ..view.clone()
        },
    ] {
        assert!(badge!(&workspace, &wrong).is_none());
    }
    workspace.cwd = "/remote/other-checkout".into();
    assert!(
        badge!(&workspace, &view).is_none(),
        "another checkout cannot borrow the answer"
    );
}
