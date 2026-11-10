//! `POST /api/v2/workspaces/{workspace}/terminals`: a first tab in a
//! workspace, which has no pane to ask for one.

use super::*;

#[tokio::test]
async fn a_new_terminal_needs_a_session_and_a_known_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path());
    let new = |workspace: &str| {
        post(
            &format!("/api/v2/workspaces/{workspace}/terminals"),
            json!({}),
        )
    };
    assert_eq!(send(&state, new("w1")).await.0, StatusCode::UNAUTHORIZED);
    let (_, token) = signed_in(&state, 7);
    let (status, _, _) = send(&state, terminal::authorized(new("w9"), &token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
