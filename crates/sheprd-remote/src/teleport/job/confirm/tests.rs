use super::*;
use crate::teleport::{
    launch::AgentKind,
    snapshot::{AgentSession, SessionKind},
};

#[test]
fn reviewed_session_flags_directory_and_provenance_must_still_match() {
    for agent in AgentKind::ALL.into_iter().filter(|agent| agent.carries()) {
        let expected = PanePlan {
            binding: None,
            pane_id: "pane".into(),
            cwd: Some("/checkout".into()),
            action: Action::Resume(agent),
            work: Work::Session {
                agent,
                session: AgentSession {
                    agent: agent.name().into(),
                    kind: SessionKind::Id,
                    source: format!("herdr:{}", agent.name()),
                    value: "session-1".into(),
                },
                flags: vec!["--model=selected".into()],
            },
        };
        assert!(unchanged(&expected, &expected.work, Some("/checkout")).is_ok());
        assert!(matches!(
            unchanged(&expected, &expected.work, Some("/elsewhere")),
            Err(Error::SourceSessionChanged)
        ));
        let Work::Session {
            agent,
            session,
            flags,
        } = &expected.work
        else {
            unreachable!()
        };
        let mut changed = session.clone();
        changed.source = "plugin:unverified".into();
        for work in [
            Work::Shell,
            Work::Session {
                agent: *agent,
                session: changed,
                flags: flags.clone(),
            },
            Work::Session {
                agent: *agent,
                session: session.clone(),
                flags: Vec::new(),
            },
        ] {
            assert!(matches!(
                unchanged(&expected, &work, Some("/checkout")),
                Err(Error::SourceSessionChanged)
            ));
        }
    }
}
