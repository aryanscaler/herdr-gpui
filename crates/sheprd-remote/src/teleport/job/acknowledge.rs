//! Input submission is not a resume acknowledgement. Require the exact
//! transferred reference from the destination's own session evidence.

use super::*;
use crate::teleport::{
    AgentKind,
    snapshot::{AgentSession, SessionKind, SnapshotResult},
};
use std::time::Instant;

pub(super) struct Resume {
    agent: AgentKind,
    kind: SessionKind,
    reference: String,
}

impl Resume {
    pub(super) fn new(agent: AgentKind, reference: String) -> Self {
        let kind = if matches!(agent, AgentKind::Pi | AgentKind::Omp) {
            SessionKind::Path
        } else {
            SessionKind::Id
        };
        Self {
            agent,
            kind,
            reference,
        }
    }

    fn matches(&self, session: Option<&AgentSession>) -> bool {
        session.is_some_and(|session| {
            self.agent.accepts(session)
                && session.kind == self.kind
                && session.value == self.reference
        })
    }
}

pub(super) fn wait(
    host: &Host,
    target: &Target,
    resume: &Resume,
    label: &str,
    cancelled: &AtomicBool,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let matched = match target.generation {
            Generation::Herdr1 => {
                let snapshot: SnapshotResult =
                    host.herdr(Step::Launch, &["api", "snapshot"], cancelled)?;
                snapshot.snapshot.pane(&target.pane_id).is_some_and(|pane| {
                    pane.agent.as_deref().and_then(AgentKind::parse) == Some(resume.agent)
                        && matches!(
                            pane.status,
                            crate::teleport::snapshot::PaneStatus::Idle
                                | crate::teleport::snapshot::PaneStatus::Done
                        )
                        && resume.matches(pane.agent_session.as_ref())
                })
            }
            Generation::Herdr2 => {
                let terminals =
                    core::terminals(host, std::slice::from_ref(&target.pane_id), cancelled)?;
                let sessions = core::sessions(host, &terminals, cancelled)?;
                terminals
                    .get(&target.pane_id)
                    .is_some_and(core::Terminal::idle)
                    && resume.matches(sessions.sessions.get(&target.pane_id))
            }
        };
        if matched {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::SessionResumeUnconfirmed {
                agent: resume.agent.binary(),
                host: label.into(),
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests;
