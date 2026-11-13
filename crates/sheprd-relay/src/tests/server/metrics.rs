//! The Prometheus exposition.

use crate::metrics::{Dial, Direction, End, Gauge, Kind, Metrics};
use std::time::Duration;

#[test]
fn every_family_is_rendered_with_help_and_type() {
    let metrics = Metrics::default();
    metrics.raise(Gauge::Connections(Kind::Dial));
    metrics.dial(Dial::Spliced);
    metrics.carried(Direction::FromDialer, 1500);
    metrics.ended(End::Closed, Duration::from_secs(30));
    let text = metrics.render();
    for line in [
        "sheprd_relay_connections{kind=\"dial\"} 1",
        "sheprd_relay_dials_total{outcome=\"spliced\"} 1",
        "sheprd_relay_bytes_total{direction=\"from_dialer\"} 1500",
        "sheprd_relay_frames_total{direction=\"from_dialer\"} 1",
        "sheprd_relay_sessions_ended_total{reason=\"closed\"} 1",
        "sheprd_relay_session_duration_seconds_bucket{le=\"10\"} 0",
        "sheprd_relay_session_duration_seconds_bucket{le=\"60\"} 1",
        "sheprd_relay_session_duration_seconds_bucket{le=\"+Inf\"} 1",
        "sheprd_relay_session_duration_seconds_sum 30",
        "sheprd_relay_session_duration_seconds_count 1",
    ] {
        assert!(
            text.lines().any(|candidate| candidate == line),
            "missing {line}\n{text}"
        );
    }
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let name = line.split(['{', ' ']).next().unwrap();
        let family = name
            .trim_end_matches("_bucket")
            .trim_end_matches("_sum")
            .trim_end_matches("_count");
        assert!(
            text.contains(&format!("# TYPE {family} ")),
            "{name} has no TYPE line"
        );
    }
}

#[test]
fn gauges_never_go_below_zero() {
    let metrics = Metrics::default();
    metrics.lower(Gauge::Sessions);
    assert_eq!(metrics.read(Gauge::Sessions), 0);
}
