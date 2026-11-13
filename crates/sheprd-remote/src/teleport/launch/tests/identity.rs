use super::*;

#[test]
fn resumed_flags_leave_paths_outside_the_checkout_alone() {
    let flags = strings(&[
        "--settings",
        "/backup/work/app/settings.json",
        "--settings=/backup/work/app/settings.json",
        "--add-dir=/work/application",
        "--settings=/work/app/settings.json",
    ]);
    assert_eq!(
        resume_command(
            AgentKind::Claude,
            "session-1",
            &flags,
            "/work/app",
            "/dest/app"
        ),
        strings(&[
            "claude",
            "--resume",
            "session-1",
            "--settings",
            "/backup/work/app/settings.json",
            "--settings=/backup/work/app/settings.json",
            "--add-dir=/work/application",
            "--settings=/dest/app/settings.json"
        ])
    );
}

#[test]
fn resumed_flags_move_but_destination_session_paths_do_not_move_twice() {
    assert_eq!(
        resume_command(
            AgentKind::Pi,
            "/work/moved/session.jsonl",
            &strings(&[
                "--settings=/work/config.json",
                "--add-dir",
                "/work/lib",
                "--model",
                "selected"
            ]),
            "/work",
            "/work/moved",
        ),
        strings(&[
            "pi",
            "--session",
            "/work/moved/session.jsonl",
            "--settings=/work/moved/config.json",
            "--add-dir",
            "/work/moved/lib",
            "--model",
            "selected"
        ])
    );
}

#[test]
fn a_session_must_belong_to_the_detected_agent() {
    for agent in AgentKind::ALL.into_iter().filter(|agent| agent.carries()) {
        let recorded = session(agent.name(), SessionKind::Id, "session-1");
        for detected in AgentKind::ALL.into_iter().filter(|other| *other != agent) {
            assert!(matches!(
                Work::classify(
                    &pane(Some(detected.name()), Some(recorded.clone())),
                    &running(&[detected.binary()]),
                ),
                Work::Agent { .. }
            ));
        }
        assert!(matches!(
            Work::classify(&pane(None, Some(recorded)), &ProcessInfo::default()),
            Work::Shell
        ));
    }
}
