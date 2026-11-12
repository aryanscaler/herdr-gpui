use super::*;
use crate::merged_cleanup::kept::kept_candidates;
use sheprd_session::Keeper;

#[test]
fn invalid_local_repository_keys_never_reach_the_pr_cache() {
    let mut workspace = kept::kept("kept-lane", true, Keeper::App);
    let invalid: &[&str] = if cfg!(windows) {
        &["", "relative/.git", "/src/app/.git", "C:src/app/.git"]
    } else {
        &["", "relative/.git"]
    };
    for &key in invalid {
        workspace.worktree.as_mut().unwrap().key = key.into();
        assert_eq!(
            kept_candidates(
                std::slice::from_ref(&workspace),
                |_| -> Option<&PullRequest> {
                    panic!("invalid repository key reached the PR cache: {key}");
                }
            )
            .count(),
            0,
            "{key}"
        );
    }
}
