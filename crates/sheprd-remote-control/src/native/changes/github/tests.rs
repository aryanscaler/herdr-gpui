use super::*;
use std::process::Stdio;

impl Reader {
    pub(crate) fn fixture(cwd: &str) -> Self {
        let reader = Self::new(true);
        populate(&reader, cwd);
        reader.cache.lock().unwrap().entries[0].logs.push((
            123,
            Ok(CheckLog {
                excerpt: "<script>untrusted log</script>".into(),
            }),
        ));
        reader
    }
}

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "feature"]);
    git(dir.path(), &["commit", "--allow-empty", "-m", "fixture"]);
    dir
}

fn git(path: &Path, args: &[&str]) {
    assert!(
        sheprd_client::new_command("git")
            .arg("-C")
            .arg(path)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.com",
                "-c",
                "commit.gpgsign=false"
            ])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
}

fn pr() -> PullRequest {
    serde_json::from_value(serde_json::json!({
        "number": 42, "url": "https://github.com/example/project/pull/42",
        "id": "PR_fixture", "headRefOid": "0123456789abcdef0123456789abcdef01234567",
        "title": "Standalone", "state": "OPEN", "isDraft": false,
        "headRefName": "feature", "baseRefName": "main", "additions": 1,
        "deletions": 0, "changedFiles": 1, "updatedAt": "2026-11-10T00:00:00Z",
        "mergeStateStatus": "BLOCKED", "headRepositoryOwner": {"login": "example"},
        "statusCheckRollup": [
            {"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE", "databaseId": 123,
             "detailsUrl": "https://github.com/example/project/actions/runs/1/job/123"},
            {"__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "SUCCESS", "databaseId": 124}
        ]
    })).unwrap()
}

fn populate(reader: &Reader, cwd: &str) {
    reader
        .with_entry_using(
            cwd,
            |input| {
                assert_eq!(input.branch, "feature");
                assert_eq!(input.checkout.as_deref(), Some(cwd));
                Ok(Some(pr()))
            },
            |_, _| Ok(()),
        )
        .unwrap();
}

#[test]
fn standalone_checks_are_real_answers_and_only_failing_jobs_are_readable() {
    let dir = repository();
    let cwd = dir.path().to_str().unwrap();
    let reader = Reader::new(true);
    let Answer::Checks(answer) = reader
        .answer_using(
            cwd,
            &Wanted::Checks,
            |_| Ok(Some(pr())),
            |_, _| panic!("checks must not fetch logs"),
        )
        .unwrap()
    else {
        panic!("checks")
    };
    assert_eq!(answer.number, Some(42));
    assert!(answer.valid());
    assert_eq!(answer.checks[0].name, "test");
    assert_eq!(answer.checks[0].job, Some(123));
    assert_eq!(answer.checks[1].job, None);
    assert_eq!(
        reader.answer(cwd, &Wanted::CheckLog { job: 124 }),
        Err(Failure::NoLog)
    );
    assert_eq!(
        reader.answer(cwd, &Wanted::CheckLog { job: 999 }),
        Err(Failure::NoLog)
    );
    reader.cache.lock().unwrap().next = None;
    let log = reader
        .answer_using(
            cwd,
            &Wanted::CheckLog { job: 123 },
            |_| panic!("cached PR"),
            |target, job| {
                assert_eq!(
                    (&*target.owner, &*target.repo, job),
                    ("example", "project", 123)
                );
                Ok("\u{1b}[31mtest failed\u{1b}[0m".into())
            },
        )
        .unwrap();
    assert_eq!(
        log,
        Answer::CheckLog(CheckLog {
            excerpt: "test failed".into()
        })
    );
    assert_eq!(
        reader.answer(cwd, &Wanted::CheckLog { job: 123 }),
        Ok(Answer::CheckLog(CheckLog {
            excerpt: "test failed".into()
        }))
    );
}

#[test]
fn disabled_reader_does_not_probe_credentials_or_checkout() {
    let reader = Reader::default();
    assert_eq!(
        reader.with_entry_using(
            "/not/a/checkout",
            |_| panic!("credential read"),
            |_, _| Ok(())
        ),
        Err(Failure::GitHubSignIn)
    );
    assert_eq!(
        reader.answer("/not/a/checkout", &Wanted::Checks),
        Err(Failure::GitHubSignIn)
    );
}

#[test]
fn no_pr_is_empty_only_after_a_successful_lookup_and_failures_are_cached() {
    let dir = repository();
    let cwd = dir.path().to_str().unwrap();
    let reader = Reader::new(true);
    reader
        .with_entry_using(cwd, |_| Ok(None), |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        reader.answer(cwd, &Wanted::Checks),
        Ok(Answer::Checks(Checks::default()))
    );
    reader
        .with_entry_using(cwd, |_| panic!("cache miss"), |_, _| Ok(()))
        .unwrap();
    let reader = Reader::new(true);
    reader
        .with_entry_using(cwd, |_| Err(sheprd_github::Error::Forbidden), |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        reader.answer(cwd, &Wanted::Checks),
        Err(Failure::GitHubScope)
    );
}

#[test]
fn changed_branch_and_commit_never_reuse_checks_or_logs() {
    let dir = repository();
    let cwd = dir.path().to_str().unwrap();
    let reader = Reader::new(true);
    populate(&reader, cwd);
    git(dir.path(), &["checkout", "-b", "other"]);
    assert_eq!(reader.answer(cwd, &Wanted::Checks), Err(Failure::Gone));
    reader.cache.lock().unwrap().next = None;
    reader
        .with_entry_using(
            cwd,
            |input| {
                assert_eq!(input.branch, "other");
                Ok(None)
            },
            |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        reader.answer(cwd, &Wanted::Checks),
        Ok(Answer::Checks(Checks::default()))
    );
    git(dir.path(), &["commit", "--allow-empty", "-m", "new head"]);
    assert_eq!(reader.answer(cwd, &Wanted::Checks), Err(Failure::Gone));
}

#[test]
fn concurrent_reads_and_account_wide_backoff_are_bounded() {
    let reader = Reader::new(true);
    let _held = reader.cache.lock().unwrap();
    assert_eq!(
        reader.answer("/unused", &Wanted::Checks),
        Err(Failure::RateLimited)
    );
    let mut cache = Cache::default();
    let now = Instant::now();
    assert_eq!(cache.reserve(now), Ok(()));
    assert_eq!(cache.reserve(now), Err(Failure::RateLimited));
    assert_eq!(cache.reserve(now + PACE), Ok(()));
    cache.pause(&sheprd_github::Error::RateLimit { reset: None });
    assert_eq!(cache.reserve(now + TTL), Err(Failure::RateLimited));
}

#[test]
fn a_branch_switched_during_lookup_cannot_publish_its_result() {
    let dir = repository();
    let cwd = dir.path().to_str().unwrap();
    let reader = Reader::new(true);
    let result = reader.with_entry_using(
        cwd,
        |_| {
            git(dir.path(), &["checkout", "-b", "replacement"]);
            Ok(Some(pr()))
        },
        |_, _| Ok(()),
    );
    assert_eq!(result, Err(Failure::Gone));
    assert!(reader.cache.lock().unwrap().entries.is_empty());
}

#[test]
fn expired_entries_and_remote_changes_require_a_new_lookup() {
    let dir = repository();
    let cwd = dir.path().to_str().unwrap();
    let reader = Reader::new(true);
    populate(&reader, cwd);
    assert!(matches!(
        reader.pull_request(cwd, "wrong-branch"),
        Err(Failure::Gone)
    ));
    {
        let mut cache = reader.cache.lock().unwrap();
        cache.entries[0].until = Instant::now();
        cache.next = None;
    }
    reader
        .with_entry_using(cwd, |_| Ok(None), |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        reader.answer(cwd, &Wanted::Checks),
        Ok(Answer::Checks(Checks::default()))
    );
    git(
        dir.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/other/repo.git",
        ],
    );
    assert_eq!(reader.answer(cwd, &Wanted::Checks), Err(Failure::Gone));
}

#[test]
fn effective_includes_and_arbitrary_remotes_invalidate_answers() {
    let dir = repository();
    let cwd = dir.path().to_str().unwrap();
    let include = dir.path().join("included-config");
    std::fs::write(
        &include,
        "[remote \"fork\"]\nurl = https://github.com/first/repo.git\n",
    )
    .unwrap();
    git(
        dir.path(),
        &["config", "include.path", include.to_str().unwrap()],
    );
    let reader = Reader::new(true);
    populate(&reader, cwd);
    std::fs::write(
        &include,
        "[remote \"fork\"]\nurl = https://github.com/second/repo.git\n",
    )
    .unwrap();
    assert_eq!(reader.answer(cwd, &Wanted::Checks), Err(Failure::Gone));
}

#[test]
fn remote_changes_during_pr_or_log_reads_discard_the_answer_and_cache() {
    for log in [false, true] {
        let dir = repository();
        let cwd = dir.path().to_str().unwrap();
        let reader = Reader::new(true);
        if log {
            populate(&reader, cwd);
            reader.cache.lock().unwrap().next = None;
        }
        let result = reader.answer_using(
            cwd,
            &if log {
                Wanted::CheckLog { job: 123 }
            } else {
                Wanted::Checks
            },
            |_| {
                git(
                    dir.path(),
                    &[
                        "remote",
                        "add",
                        "fork",
                        "https://github.com/replaced/repo.git",
                    ],
                );
                Ok(Some(pr()))
            },
            |_, _| {
                git(
                    dir.path(),
                    &[
                        "remote",
                        "add",
                        "fork",
                        "https://github.com/replaced/repo.git",
                    ],
                );
                Ok("old log".into())
            },
        );
        assert_eq!(result, Err(Failure::Gone));
        assert!(reader.cache.lock().unwrap().entries.is_empty());
    }
}

#[test]
fn temporary_failures_keep_a_matching_badge_but_absence_and_identity_changes_remove_it() {
    assert!(!replaces_badge(&Ok(Err(Failure::RateLimited))));
    assert!(!replaces_badge(&Ok(Err(Failure::GitHubUnavailable))));
    assert!(replaces_badge(&Ok(Ok(None))));
    assert!(replaces_badge(&Ok(Err(Failure::Gone))));
    assert!(replaces_badge(&Ok(Ok(Some(pr())))));
}

#[test]
fn remote_changes_clear_old_badges_even_during_account_backoff() {
    let dir = repository();
    let cwd = dir.path().to_str().unwrap();
    let reader = Reader::new(true);
    populate(&reader, cwd);
    reader
        .cache
        .lock()
        .unwrap()
        .pause(&sheprd_github::Error::RateLimit { reset: None });
    git(
        dir.path(),
        &["remote", "add", "fork", "https://github.com/new/repo.git"],
    );
    let answer = reader.pull_request(cwd, "feature");
    assert!(matches!(answer, Err(Failure::Gone)));
    assert!(replaces_badge(&Ok(answer)));
    assert!(reader.cache.lock().unwrap().entries.is_empty());
    // Invalidating the presentation never bypasses the actual account pause.
    assert_eq!(
        reader.answer(cwd, &Wanted::Checks),
        Err(Failure::GitHubPending)
    );
    // An on-demand read may have consumed the invalidation before the
    // background badge tick. Absence of a verified identity still clears it.
    assert!(replaces_badge(&Ok(reader.pull_request(cwd, "feature"))));
}

#[test]
fn an_expired_but_matching_identity_preserves_its_badge_during_backoff() {
    let dir = repository();
    let cwd = dir.path().to_str().unwrap();
    let reader = Reader::new(true);
    populate(&reader, cwd);
    {
        let mut cache = reader.cache.lock().unwrap();
        cache.entries[0].until = Instant::now();
        cache.pause(&sheprd_github::Error::RateLimit { reset: None });
    }
    let answer = reader.pull_request(cwd, "feature");
    assert!(matches!(answer, Err(Failure::RateLimited)));
    assert!(!replaces_badge(&Ok(answer)));
}
