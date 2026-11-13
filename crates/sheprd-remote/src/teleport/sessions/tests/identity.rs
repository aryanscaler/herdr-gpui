use super::*;

#[test]
fn source_config_root_is_read_and_mapped_for_transfer_and_resume() {
    use sheprd_client::herdr::sessions::{Binding, Identity, reference};
    use std::collections::BTreeMap;
    let homes = homes();
    let source_root = homes.source_home.join("custom-codex");
    let relative = "sessions/2026/11/11/rollout-date-session-1.jsonl";
    let path = source_root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "selected custom session\n").unwrap();
    let binding = Binding {
        version: 1,
        sequence: 1,
        identity: Identity {
            core: "core".into(),
            terminal: "t1".into(),
            pid: 42,
            started: "opaque".into(),
        },
        agent: "codex".into(),
        session: Some(reference(AgentKind::Codex, SessionKind::Id, "session-1".into()).unwrap()),
        environment: BTreeMap::from([(
            "CODEX_HOME".into(),
            source_root.to_string_lossy().into_owned(),
        )]),
    };
    let environment = Environment::map(
        &homes.source,
        &homes.destination,
        Some(&binding),
        FROM,
        TO,
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut route = route(&homes, TO);
    route.environment = Some(&environment);
    move_session(
        AgentKind::Codex,
        binding.selected().unwrap(),
        &route,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(homes.destination_home.join("custom-codex").join(relative)).unwrap(),
        "selected custom session\n"
    );
    assert!(
        !homes.destination_home.join(".codex").exists(),
        "never silently use a different session store"
    );
}

#[test]
fn transfers_refuse_mismatched_agents_and_untrusted_provenance() {
    let homes = homes();
    for agent in AgentKind::ALL.into_iter().filter(|agent| agent.carries()) {
        for other in AgentKind::ALL.into_iter().filter(|other| *other != agent) {
            assert!(matches!(
                move_session(
                    agent,
                    &session(other.name(), SessionKind::Id, "session-1"),
                    &route(&homes, TO),
                    &AtomicBool::new(false)
                ),
                Err(Error::SessionMissing { .. })
            ));
        }
        let mut forged = session(agent.name(), SessionKind::Id, "session-1");
        forged.source = "plugin:unverified".into();
        assert!(matches!(
            move_session(agent, &forged, &route(&homes, TO), &AtomicBool::new(false)),
            Err(Error::SessionMissing { .. })
        ));
    }
    assert!(
        fs::read_dir(&homes.destination_home)
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn cancellation_also_applies_to_server_backed_sessions() {
    let homes = homes();
    assert!(matches!(
        move_session(
            AgentKind::Letta,
            &session("letta", SessionKind::Id, "default:agent-1"),
            &route(&homes, TO),
            &AtomicBool::new(true)
        ),
        Err(Error::Cancelled)
    ));
}

#[test]
fn duplicate_session_files_are_not_arbitrarily_selected() {
    for (agent, paths) in [
        (
            AgentKind::Claude,
            [
                ".claude/projects/a/session-1.jsonl",
                ".claude/projects/b/session-1.jsonl",
            ],
        ),
        (
            AgentKind::Codex,
            [
                ".codex/sessions/a/rollout-a-session-1.jsonl",
                ".codex/sessions/b/rollout-b-session-1.jsonl",
            ],
        ),
        (
            AgentKind::Pi,
            [
                ".pi/agent/sessions/a/time_session-1.jsonl",
                ".pi/agent/sessions/b/time_session-1.jsonl",
            ],
        ),
        (
            AgentKind::Omp,
            [
                ".omp/agent/sessions/a/time_session-1.jsonl",
                ".omp/agent/sessions/b/time_session-1.jsonl",
            ],
        ),
    ] {
        let homes = homes();
        for path in paths {
            let path = homes.source_home.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "{}\n").unwrap();
        }
        assert!(matches!(
            move_session(
                agent,
                &session(agent.name(), SessionKind::Id, "session-1"),
                &route(&homes, TO),
                &AtomicBool::new(false)
            ),
            Err(Error::SessionMissing { .. })
        ));
        assert!(
            fs::read_dir(&homes.destination_home)
                .unwrap()
                .next()
                .is_none()
        );
    }
}

#[test]
fn pi_ids_match_the_complete_filename_suffix() {
    let homes = homes();
    let dir = homes.source_home.join(".pi/agent/sessions/project");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("time_session-123.jsonl"), "other conversation\n").unwrap();
    let selected = session("pi", SessionKind::Id, "session-1");
    assert!(matches!(
        move_session(
            AgentKind::Pi,
            &selected,
            &route(&homes, TO),
            &AtomicBool::new(false)
        ),
        Err(Error::SessionMissing { .. })
    ));
    fs::write(dir.join("time_session-1.jsonl"), "selected conversation\n").unwrap();
    let moved = move_session(
        AgentKind::Pi,
        &selected,
        &route(&homes, TO),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(moved).unwrap(),
        "selected conversation\n"
    );
}
