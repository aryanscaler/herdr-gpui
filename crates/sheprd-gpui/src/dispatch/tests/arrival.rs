use super::*;
use std::time::{Duration, Instant};

#[test]
fn a_completed_job_waits_for_catalog_visibility_without_recreating_work() {
    let arrival = Dispatched::Kept {
        endpoint_id: "new-device".into(),
        host_identity: "cloud:new-device".into(),
        path: "/home/project".into(),
        label: None,
        checkout: None,
    };
    let mut job = Job::finished("New device", "project", Ok(arrival));
    let (_, result) = job.poll();
    let arrival = result.unwrap().unwrap();
    let now = Instant::now();
    assert!(job.await_destination(arrival, now).is_none());
    let (_, result) = job.poll();
    let arrival = result.unwrap().unwrap();
    assert!(
        job.await_destination(arrival, now + Duration::from_secs(119))
            .is_none()
    );
    let (_, result) = job.poll();
    let arrival = result.unwrap().unwrap();
    assert!(
        job.await_destination(arrival, now + Duration::from_secs(120))
            .is_some()
    );
    assert!(job.poll().1.is_none());
}
