use super::*;
use crate::fan_out::journal::Resume;

#[test]
fn owned_creation_and_start_intents_survive_restart_without_guessing_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fan-outs.json");
    let journal = Journal::at(Some(path.clone()));
    let record = Record::new(42, &origin(), "fix", &plan::lanes(&launched().picks, 7));
    journal.begin(record).unwrap().unwrap().write().unwrap();
    let owned = Checkout {
        worktree: work::Worktree {
            path: "/remote/lane".into(),
            opened: work::Opened::Herdr2,
        },
        recoverable: true,
        ..checkout(0)
    };
    journal
        .record(42, &Report::Lane(0, Progress::CreatingWorktree))
        .unwrap();
    journal
        .record(
            42,
            &Report::Lane(0, Progress::CreatingOwned(Box::new(owned.clone()))),
        )
        .unwrap();
    let restarted = Journal::at(Some(path.clone()));
    let restored = FanOut::reclaim(&restarted, |_, _| Some(origin().host)).unwrap();
    assert_eq!(restored.resumable(0), Some(Resume::Creation));
    assert_eq!(restored.lanes[0].checkout, Some(owned.clone()));
    journal
        .record(42, &Report::Lane(0, Progress::Created(Box::new(owned))))
        .unwrap();
    journal
        .record(42, &Report::Lane(0, Progress::StartingAgent))
        .unwrap();
    let restarted = Journal::at(Some(path));
    let mut restored = FanOut::reclaim(&restarted, |_, _| Some(origin().host)).unwrap();
    // This action only looks up the atomic ownership label, never runs
    // another agent merely because the original run reply was lost.
    assert_eq!(restored.resumable(0), Some(Resume::Prompt));
    assert_eq!(restored.poll(), (false, None));
}

#[test]
fn a_missing_terminal_after_starting_never_offers_a_second_agent() {
    for step in [
        Checkpoint::Created,
        Checkpoint::StartingAgent,
        Checkpoint::Prompting,
    ] {
        for terminal in [None, Some("t1".to_owned())] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("fan-outs.json");
            let journal = Journal::at(Some(path.clone()));
            let mut record = Record::new(
                42,
                &origin(),
                "fix the bug",
                &plan::lanes(&launched().picks, 7),
            );
            let checkout = Checkout {
                worktree: work::Worktree {
                    path: "/tmp/lane".into(),
                    opened: work::Opened::Herdr2,
                },
                terminal: terminal.clone(),
                ..checkout(0)
            };
            record.advance(&Report::Lane(0, Progress::Created(Box::new(checkout))));
            record.lanes[0].step = step;
            journal.begin(record).unwrap().unwrap().write().unwrap();
            let journal = Journal::at(Some(path));
            let mut restored = FanOut::reclaim(&journal, |_, _| Some(origin().host)).unwrap();
            let expected = match step {
                Checkpoint::Created => Some(Resume::Start),
                Checkpoint::StartingAgent if terminal.is_some() => Some(Resume::Prompt),
                _ => None,
            };
            assert_eq!(restored.resumable(0), expected);
            if expected.is_none() {
                assert!(!restored.resume(0));
                assert!(!restored.work.load(Ordering::Acquire));
            }
        }
    }
}

#[test]
fn the_resume_worker_refuses_an_unknown_terminal_too() {
    let lane = plan::lanes(&launched().picks, 7).remove(0);
    let checkout = Checkout {
        worktree: work::Worktree {
            path: "/nonexistent/fan-out-checkout".into(),
            opened: work::Opened::Herdr2,
        },
        ..checkout(0)
    };
    let reports = std::sync::Mutex::new(Vec::new());
    job::resume(
        Resume::Prompt,
        0,
        &lane,
        &checkout,
        "fix",
        &|report| {
            reports.lock().unwrap().push(report);
            Ok(())
        },
        &AtomicBool::new(false),
    );
    assert!(matches!(
        reports.lock().unwrap().as_slice(),
        [Report::Lane(0, Progress::Failed(Error::AgentUncertain))]
    ));
}
