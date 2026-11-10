//! A removed Herdr 2 run's kept workspace closes once the worker forgot
//! the run, and only then.

use super::*;
use crate::orchestrator::view::core_removal::{core_checkout, removed};
use std::path::PathBuf;

#[test]
fn only_a_local_herdr_2_run_has_a_kept_checkout_to_close() {
    let (found, session) = core(None);
    let (runs, sessions) = (vec![found], vec![session]);
    assert_eq!(
        core_checkout(&Runs::new(&runs, &sessions, &[]), 0),
        Some(PathBuf::from("/home/me/.herdr/worktrees/app/7-icons"))
    );
    let (found, session) = core(Some("devbox"));
    let (runs, sessions) = (vec![found], vec![session]);
    assert_eq!(core_checkout(&Runs::new(&runs, &sessions, &[]), 0), None);
}

#[test]
fn a_followed_removal_closes_once_its_run_is_gone() {
    let mut followed = vec![
        ("r7".to_owned(), PathBuf::from("/w/r7")),
        ("r8".to_owned(), PathBuf::from("/w/r8")),
    ];
    // Both still listed: the worker has not removed either yet.
    assert!(removed(&mut followed, |_| true).is_empty());
    assert_eq!(followed.len(), 2);
    let gone = removed(&mut followed, |run| run == "r8");
    assert_eq!(gone, [PathBuf::from("/w/r7")]);
    assert_eq!(followed, [("r8".to_owned(), PathBuf::from("/w/r8"))]);
}
