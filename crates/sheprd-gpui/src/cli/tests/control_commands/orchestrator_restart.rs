use super::*;

#[test]
fn restart_is_a_confirmation_request_scoped_to_the_callers_project() {
    for args in [
        &["orchestrator", "restart", "run-1", "--repo", "/src/app"][..],
        &["orchestrator", "--repo", "/src/app", "restart", "run-1"],
    ] {
        assert_eq!(
            command(args),
            ControlCommand::Orchestrator(control::OrchestratorCommand::Restart {
                repo: Some("/src/app".into()),
                run: "run-1".into(),
            })
        );
    }
}

#[test]
fn adoption_requests_read_only_evidence_then_user_confirmation() {
    assert_eq!(
        command(&["orchestrator", "adopt", "legacy-1"]),
        ControlCommand::Orchestrator(control::OrchestratorCommand::Adopt {
            repo: None,
            run: "legacy-1".into(),
        })
    );
}
