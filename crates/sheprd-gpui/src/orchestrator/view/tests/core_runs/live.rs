use super::*;
use crate::orchestrator::Terminals;

#[test]
fn core_terminal_status_is_scoped_to_backend_host_and_session() {
    let (run, mut session) = core(Some("remote"));
    session.session = Some("named".into());
    let agent = LiveAgent {
        terminals: Terminals::Herdr2,
        host: session.host.clone(),
        session: session.session.clone(),
        workspace_id: String::new(),
        ..live(&run, "t9", AgentStatus::Blocked)
    };
    let agents = [agent.clone()];
    assert!(live_for(&session, &agents).is_some());
    let runs = [run.clone()];
    let sessions = [session.clone()];
    assert_eq!(
        Runs::new(&runs, &sessions, &agents).status(0),
        Some(Status::NeedsInput)
    );

    for mismatch in [
        LiveAgent {
            host: None,
            ..agent.clone()
        },
        LiveAgent {
            session: None,
            ..agent.clone()
        },
        LiveAgent {
            pane_id: "t10".into(),
            ..agent.clone()
        },
        LiveAgent {
            terminals: Terminals::Herdr1,
            ..agent.clone()
        },
    ] {
        assert!(live_for(&session, &[mismatch]).is_none());
    }
    session.pane = Pane::Herdr1 {
        workspace_id: String::new(),
        pane_id: "t9".into(),
    };
    assert!(live_for(&session, &agents).is_none());
}

#[test]
fn core_projection_never_matches_the_legacy_workspace_fallback() {
    let (mut run, _) = core(None);
    if let Some(workspace) = &mut run.workspace {
        workspace.id = String::new();
    }
    let agent = LiveAgent {
        terminals: Terminals::Herdr2,
        workspace_id: String::new(),
        ..live(&run, "t9", AgentStatus::Blocked)
    };
    let runs = [run];
    let agents = [agent];
    assert!(Runs::new(&runs, &[], &agents).live(0).is_none());
}
