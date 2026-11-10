use super::*;
use crate::fan_out::journal::Resume;

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
            let mut restored = FanOut::reclaim(&journal, |_| Some(origin().host)).unwrap();
            let expected = match step {
                Checkpoint::Created => Some(Resume::Start),
                _ if terminal.is_some() => Some(Resume::Prompt),
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
