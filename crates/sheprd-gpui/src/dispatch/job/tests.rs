use super::*;

#[test]
fn closing_a_job_waiting_for_catalog_drops_the_arrival_and_cancels_its_worker() {
    let mut job = Job::finished("Box", "project", Err(super::super::Error::Busy.into()));
    let cancelled = Arc::clone(&job.cancelled);
    let arrival = Dispatched::Kept {
        endpoint_id: "box".into(),
        host_identity: "ssh:box".into(),
        path: "/home/project".into(),
        label: None,
        checkout: None,
    };
    assert!(
        job.await_destination(arrival, std::time::Instant::now())
            .is_none()
    );
    drop(job);
    assert!(cancelled.load(Ordering::Acquire));
}
