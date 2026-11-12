use super::*;
use std::time::{Duration, Instant};

#[test]
fn an_expired_or_cancelled_identity_budget_never_starts_git() {
    let fixture = Fixture::new();
    fixture.program("git", "exit 99");
    assert!(matches!(
        identity_before(
            &fixture.host(),
            fixture.repo.to_str().unwrap(),
            Instant::now(),
            &never()
        ),
        Err(Error::Script {
            source: sheprd_client::Error::ScriptTimeout,
            ..
        })
    ));
    assert!(matches!(
        identity_before(
            &fixture.host(),
            fixture.repo.to_str().unwrap(),
            Instant::now() + Duration::from_secs(5),
            &AtomicBool::new(true)
        ),
        Err(Error::Cancelled)
    ));
    assert!(fixture.calls().is_empty());
}

#[test]
fn a_blocked_identity_query_is_stopped_and_no_second_query_runs() {
    let fixture = Fixture::new();
    // A process with no external resources, blocked until the host runner
    // observes the shared deadline token and reaps it.
    fixture.program("git", "exec sleep 30");
    let result = identity_before(
        &fixture.host(),
        fixture.repo.to_str().unwrap(),
        Instant::now() + Duration::from_millis(100),
        &never(),
    );
    assert!(matches!(
        result,
        Err(Error::Script {
            source: sheprd_client::Error::ScriptTimeout,
            ..
        })
    ));
    assert!(fixture.calls().len() <= 1);
}
