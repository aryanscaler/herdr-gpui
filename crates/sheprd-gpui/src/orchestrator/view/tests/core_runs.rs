//! A Herdr 2 run has no Herdr 1 pane: no live agent stands for it, its
//! status is what it recorded, and opening it shows its kept workspace.
use super::*;
use crate::orchestrator::view::{Event, core_run};

/// A run dispatched on Herdr 2 on this machine, with its session.
fn core(host: Option<&str>) -> (Run, HerdrSession) {
    let item = item(Provider::Github, "7", "Icons", "2026-10-04T01:00:00Z");
    let mut run = run("r7", &item, RunState::Running, "2026-10-04T01:00:00Z");
    if let Some(workspace) = &mut run.workspace {
        workspace.id = String::new();
        workspace.host = host.map(str::to_owned);
        workspace.path = Some("/home/me/.herdr/worktrees/app/7-icons".into());
    }
    let session = HerdrSession {
        host: host.map(str::to_owned),
        pane: Pane::Herdr2 {
            terminal: "t9".into(),
        },
        ..session(&run, "")
    };
    (run, session)
}

#[test]
fn a_herdr_2_run_matches_no_herdr_1_agent_and_keeps_its_own_status() {
    let (found, session) = core(None);
    let runs = vec![found.clone()];
    let sessions = vec![session];
    // Herdr 2's session leaves Herdr 1's ids empty; nothing may match them.
    let agents = vec![LiveAgent {
        workspace_id: String::new(),
        ..live(&found, "", AgentStatus::Blocked)
    }];
    let listed = Runs::new(&runs, &sessions, &agents);
    assert!(listed.live(0).is_none());
    assert_eq!(listed.status(0), Some(Status::Working));
}

#[test]
fn opening_a_herdr_2_run_shows_its_checkout_and_terminal_on_this_machine() {
    let (found, session) = core(None);
    let runs = vec![found];
    let sessions = vec![session];
    let listed = Runs::new(&runs, &sessions, &[]);
    let Some(Event::OpenCoreRun {
        path,
        terminal,
        label,
    }) = core_run(&listed, 0)
    else {
        panic!("a Herdr 2 run opens its kept workspace");
    };
    assert_eq!(
        path,
        std::path::Path::new("/home/me/.herdr/worktrees/app/7-icons")
    );
    assert_eq!((terminal.as_str(), label.as_str()), ("t9", "b-r7"));

    // herdr-core runs only this machine's terminals for now.
    let (found, session) = core(Some("devbox"));
    let runs = vec![found];
    let sessions = vec![session];
    assert!(core_run(&Runs::new(&runs, &sessions, &[]), 0).is_none());
}

mod removal;
