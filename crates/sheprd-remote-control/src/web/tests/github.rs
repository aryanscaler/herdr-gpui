use super::*;
use crate::{hub::Workspace, native::changes::github::Reader};

fn request(token: &str, path: &str) -> HttpRequest<Body> {
    HttpRequest::get(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn saved_and_daemon_owned_workspaces_answer_without_desktop_or_herdr_one() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_str().unwrap();
    for args in [
        vec!["init", "-b", "feature"],
        vec!["commit", "--allow-empty", "-m", "fixture"],
    ] {
        assert!(
            sheprd_client::new_command("git")
                .arg("-C")
                .arg(cwd)
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.com",
                    "-c",
                    "commit.gpgsign=false"
                ])
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let workspaces = ["core:saved", "core:owned"]
        .map(|id| Workspace {
            id: id.into(),
            label: id.into(),
            branch: Some("feature".into()),
            cwd: cwd.into(),
            worktree: None,
            session: None,
            chats: vec![],
            terminals: vec![],
        })
        .into();
    let hub = Hub::offline(Layout {
        connected: true,
        boot: "core-only".into(),
        working: 0,
        workspaces,
    });
    let mut state = state_with(dir.path(), hub);
    Arc::get_mut(&mut state).unwrap().github = Reader::fixture(cwd);
    let (_, token) = signed_in(&state, 3);
    assert!(!state.changes.open());
    for workspace in ["core:saved", "core:owned"] {
        let path = format!("/api/v2/workspaces/{workspace}/checks");
        let (status, _, body) = send(&state, request(&token, &path)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let checks: crate::control::changes::Checks = serde_json::from_str(&body).unwrap();
        assert_eq!(checks.number, Some(42));
        assert_eq!(checks.checks[0].job, Some(123));
        assert!(state.changes.take(std::time::Duration::ZERO).0.is_empty());
    }
    let (status, ..) = send(&state, request(&token, "/api/v2/workspaces/unknown/checks")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let mut updates = state.pull_requests.subscribe();
    let prefetch = tokio::spawn(crate::native::changes::github::badges(state.clone()));
    tokio::time::timeout(std::time::Duration::from_secs(3), updates.changed())
        .await
        .unwrap()
        .unwrap();
    prefetch.abort();
    // Desktop publication may contribute +PR hints without replacing the
    // daemon's independently fetched badge (its PR list here is empty).
    let (stop, _) = tokio::sync::watch::channel(false);
    let hint = crate::control::pull_requests::UnopenedBranch {
        workspace_id: "core:owned".into(),
        cwd: cwd.into(),
        branch: "feature".into(),
        commits: 3,
    };
    let response = crate::server::answer_for_tests(
        &state,
        &stop,
        crate::control::Request::PullRequests {
            snapshot: crate::control::pull_requests::PullRequests {
                boot_id: "core-only".into(),
                entries: vec![],
                unopened: vec![hint.clone()],
            },
        },
    );
    assert_eq!(response, crate::control::Response::Done);
    assert_eq!(state.pull_requests.borrow().unopened, vec![hint]);
    let (status, _, body) = send(&state, request(&token, "/api/v2/home")).await;
    assert_eq!(status, StatusCode::OK);
    let home: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(home["github"], true);
    assert_eq!(home["workspaces"][0]["pull_request"]["number"], 42);
    assert_eq!(home["workspaces"][1]["unopened_commits"], 3);
    for path in ["/f/checks/core:saved", "/f/checks/core:saved/123"] {
        let (status, ..) = send(&state, get(path)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _, body) = send(&state, request(&token, path)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if path.ends_with("123") {
            assert!(
                body.contains("&lt;script&gt;untrusted log&lt;/script&gt;"),
                "{body}"
            );
            assert!(!body.contains("<script>"));
            assert!(body.contains("wrap-anywhere"));
            assert!(
                include_str!("../assets/app.css")
                    .contains(".wrap-anywhere{overflow-wrap:anywhere}")
            );
        } else {
            assert!(body.contains("Pull request #42"), "{body}");
            assert!(body.contains("Read failing log"));
        }
    }
}
