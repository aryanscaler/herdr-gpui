use super::*;
use crate::fan_out::journal::Resume;

#[test]
fn a_missing_host_can_rejoin_the_comparison_without_replaying_any_work() {
    let journal = Journal::default();
    let lanes = plan::lanes(&launched().picks, 7);
    let mut record = Record::new(42, &origin(), "go", &lanes);
    let mut remote = checkout(0);
    remote.endpoint_id = "saved-host".into();
    record.advance(&Report::Lane(
        0,
        Progress::Created(Box::new(remote.clone())),
    ));
    let _ = journal.begin(record.clone()).unwrap();
    let mut restored = FanOut::restore(record, journal, |id, _| {
        (id == "local").then(|| origin().host)
    })
    .unwrap();
    assert!(restored.lanes[0].checkout.is_none());
    let different = crate::teleport::host_for(&sheprd_client::ConnectTarget::Session {
        name: "different".into(),
        development: false,
    })
    .unwrap();
    assert!(!restored.reconnect(|_, _| Some(different.clone())));
    assert!(restored.reconnect(|_, _| Some(origin().host)));
    assert_eq!(restored.lanes[0].checkout, Some(remote));
    assert_eq!(restored.resumable(0), Some(Resume::Start));
    assert_eq!(restored.poll(), (false, None));
}

#[test]
fn an_unfinished_record_can_be_reclaimed_after_its_window_releases_it() {
    let journal = Journal::default();
    let lanes = plan::lanes(&launched().picks, 7);
    let mut record = Record::new(42, &origin(), "go", &lanes);
    record.advance(&Report::Lane(0, Progress::Created(Box::new(checkout(0)))));
    let _ = journal.begin(record.clone()).unwrap();
    let restored = FanOut::restore(record, journal.clone(), |_, _| Some(origin().host)).unwrap();
    assert!(journal.claim(|_| true).is_none());
    drop(restored);
    assert!(FanOut::reclaim(&journal, |_, _| Some(origin().host)).is_some());
}

#[test]
fn custom_herdr_socket_keeps_core_recovery_local_without_rerouting_herdr_1() {
    let target = sheprd_client::ConnectTarget::Socket("/custom/herdr.sock".into());
    let host = |_: &str, keeper| match keeper {
        sheprd_session::Keeper::App => work::host_for_kept(&target).ok(),
        sheprd_session::Keeper::Herdr1 => crate::teleport::host_for(&target).ok(),
    };
    let lanes = plan::lanes(&launched().picks, 7);
    let kept_origin = Origin {
        checkout: Some("/repo".into()),
        ..origin()
    };
    let mut kept = Record::new(42, &kept_origin, "go", &lanes);
    let tree = Checkout {
        worktree: work::Worktree {
            path: "/repo/lane".into(),
            opened: work::Opened::Herdr2,
        },
        ..checkout(0)
    };
    kept.advance(&Report::Lane(0, Progress::Created(Box::new(tree))));
    assert!(FanOut::restore(kept, Journal::default(), host).is_some());
    let mut daemon = Record::new(43, &origin(), "go", &lanes);
    daemon.advance(&Report::Lane(0, Progress::Created(Box::new(checkout(0)))));
    assert!(FanOut::restore(daemon, Journal::default(), host).is_none());
}
