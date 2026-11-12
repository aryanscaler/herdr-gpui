//! A removed Herdr 2 run's kept workspace closes once the worker forgot
//! the run, and only then.

use super::*;
use crate::orchestrator::view::core_removal::{Checkout, core_checkout, removed};
use std::path::PathBuf;

#[test]
fn a_herdr_2_checkout_keeps_its_host_when_closed() {
    let (found, session) = core(None);
    let (runs, sessions) = (vec![found], vec![session]);
    assert_eq!(
        core_checkout(&Runs::new(&runs, &sessions, &[]), 0),
        Some(Checkout {
            target: crate::orchestrator::actions::target_of(&sessions[0]),
            path: PathBuf::from("/home/me/.herdr/worktrees/app/7-icons"),
        })
    );
    let (found, session) = core(Some("devbox"));
    let (runs, sessions) = (vec![found], vec![session]);
    let checkout = core_checkout(&Runs::new(&runs, &sessions, &[]), 0).unwrap();
    assert!(
        matches!(checkout.target, sheprd_client::ConnectTarget::Ssh { target, .. } if target == "devbox")
    );
}

#[test]
fn a_followed_removal_closes_once_its_run_is_gone() {
    let checkout = |path: &str| Checkout {
        target: sheprd_client::ConnectTarget::Local,
        path: path.into(),
    };
    let mut followed = vec![
        ("r7".to_owned(), checkout("/w/r7")),
        ("r8".to_owned(), checkout("/w/r8")),
    ];
    // Both still listed: the worker has not removed either yet.
    assert!(removed(&mut followed, |_| true).is_empty());
    assert_eq!(followed.len(), 2);
    let gone = removed(&mut followed, |run| run == "r8");
    assert_eq!(gone, [checkout("/w/r7")]);
    assert_eq!(followed, [("r8".to_owned(), checkout("/w/r8"))]);
}
