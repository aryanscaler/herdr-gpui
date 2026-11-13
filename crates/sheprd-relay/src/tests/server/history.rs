//! Throughput history: rates from totals, bounded, and safe across a reset.

use crate::dashboard::history::{History, SAMPLES, Sample};

#[test]
fn totals_become_rates() {
    let mut history = History::default();
    history.record(0, 0, 0);
    assert!(history.samples().is_empty());
    history.record(5_000, 10_000, 50_000);
    assert_eq!(
        history.samples(),
        vec![Sample {
            at_ms: 5_000,
            from_dialers: 2_000,
            from_listeners: 10_000,
        }]
    );
    // Totals that went back count as no traffic, not a huge rate.
    history.record(10_000, 0, 0);
    assert_eq!(history.samples()[1].from_dialers, 0);
}

#[test]
fn only_ten_minutes_are_kept() {
    let mut history = History::default();
    for step in 0..(SAMPLES as u64 + 10) {
        history.record(step * 5_000, step, step);
    }
    let samples = history.samples();
    assert_eq!(samples.len(), SAMPLES);
    assert!(samples.windows(2).all(|pair| pair[0].at_ms < pair[1].at_ms));
}
