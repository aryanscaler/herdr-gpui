#![allow(clippy::unwrap_used)]
use super::*;

fn fixture(agent: AgentKind) -> Binding {
    Binding {
        version: 1,
        sequence: 1,
        identity: Identity {
            core: "core-a".into(),
            terminal: "t1".into(),
            pid: 42,
            started: "linux:123".into(),
        },
        agent: agent.name().into(),
        session: Some(reference(agent, SessionKind::Id, "session-1".into()).unwrap()),
        environment: BTreeMap::new(),
    }
}

#[test]
fn all_carried_agents_accept_explicit_sheprd_evidence() {
    for agent in AgentKind::ALL.into_iter().filter(|agent| agent.carries()) {
        let binding = fixture(agent);
        let bytes = serde_json::to_vec(&binding).unwrap();
        let decoded = Binding::parse(&bytes, &binding.identity, agent).unwrap();
        assert_eq!(decoded.selected(), binding.selected());
        assert_eq!(decoded.selected().unwrap().source, SOURCE);
    }
}

#[test]
fn scope_and_process_birth_are_all_required() {
    let binding = fixture(AgentKind::Codex);
    for expected in [
        Identity {
            core: "other-host".into(),
            ..binding.identity.clone()
        },
        Identity {
            terminal: "t2".into(),
            ..binding.identity.clone()
        },
        Identity {
            pid: 43,
            ..binding.identity.clone()
        },
        Identity {
            started: "linux:124".into(),
            ..binding.identity.clone()
        },
    ] {
        assert!(matches!(
            binding.validate(&expected, AgentKind::Codex),
            Err(Error::Identity)
        ));
    }
}

#[test]
fn only_config_paths_cross_the_binding_boundary() {
    let mut binding = fixture(AgentKind::Codex);
    binding
        .environment
        .insert("CODEX_HOME".into(), "/custom/codex".into());
    assert!(
        binding
            .validate(&binding.identity, AgentKind::Codex)
            .is_ok()
    );
    binding
        .environment
        .insert("API_KEY".into(), "private".into());
    assert!(matches!(
        binding.validate(&binding.identity, AgentKind::Codex),
        Err(Error::Environment)
    ));
}

#[test]
fn configuration_roots_and_windows_session_paths_are_unambiguous() {
    let mut binding = fixture(AgentKind::Codex);
    binding
        .environment
        .insert("CODEX_HOME".into(), "relative/config".into());
    assert!(matches!(
        binding.validate(&binding.identity, AgentKind::Codex),
        Err(Error::Environment)
    ));
    binding
        .environment
        .insert("CODEX_HOME".into(), "C:\\config\\codex".into());
    assert!(
        binding
            .validate(&binding.identity, AgentKind::Codex)
            .is_ok()
    );
    assert!(
        reference(
            AgentKind::Pi,
            SessionKind::Path,
            "C:\\sessions\\exact.jsonl".into()
        )
        .is_ok()
    );
    assert!(reference(AgentKind::Pi, SessionKind::Path, "C:relative.jsonl".into()).is_err());
}

#[test]
fn clear_version_bounds_and_provenance_are_not_guessed() {
    let mut binding = fixture(AgentKind::Opencode);
    binding.session = None;
    assert_eq!(binding.selected(), None);
    binding.version = 2;
    assert!(matches!(
        binding.validate(&binding.identity, AgentKind::Opencode),
        Err(Error::Version)
    ));
    assert!(matches!(
        Binding::parse(
            &vec![b' '; MAX_RECORD + 1],
            &binding.identity,
            AgentKind::Opencode
        ),
        Err(Error::TooLarge)
    ));
    let mut binding = fixture(AgentKind::Copilot);
    binding.session.as_mut().unwrap().source = "herdr:copilot".into();
    assert!(matches!(
        binding.validate(&binding.identity, AgentKind::Copilot),
        Err(Error::Session)
    ));
}
