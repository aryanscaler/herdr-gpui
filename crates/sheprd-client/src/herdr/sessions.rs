//! Process-bound session evidence shared by terminal conversion and Teleport.
//! A session list, a directory match and launch arguments are not evidence of
//! the conversation currently selected in an agent. Reporters must observe a
//! session-start event or the terminal client's selected route.

mod error;
pub mod install;

use super::{
    AgentKind,
    snapshot::{AgentSession, SessionKind},
};
pub use error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_RECORD: usize = 64 * 1024;
pub const SOURCE: &str = "sheprd:session-v1";

/// Host-local identity. `started` is the OS process birth token, not a file
/// modification time. Callers acquire it on the host that owns `terminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub core: String,
    pub terminal: String,
    pub pid: u32,
    pub started: String,
}

/// A separately installed Sheprd reporter's latest explicit selection. `None`
/// clears the selection; it must never fall back to a previous conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub version: u32,
    pub sequence: u64,
    pub identity: Identity,
    pub agent: String,
    pub session: Option<AgentSession>,
    pub environment: BTreeMap<String, String>,
}

impl Binding {
    /// Decode bounded evidence and verify its entire scope against a fresh
    /// host-local identity. This is a read fence, not an atomic core mutation.
    pub fn parse(bytes: &[u8], expected: &Identity, agent: AgentKind) -> Result<Self> {
        if bytes.len() > MAX_RECORD {
            return Err(Error::TooLarge);
        }
        let binding: Self = serde_json::from_slice(bytes)?;
        binding.validate(expected, agent)?;
        Ok(binding)
    }

    pub fn validate(&self, expected: &Identity, agent: AgentKind) -> Result<()> {
        if self.version != 1 {
            return Err(Error::Version);
        }
        if &self.identity != expected
            || expected.pid == 0
            || expected.core.is_empty()
            || expected.terminal.is_empty()
            || expected.started.is_empty()
            || self.agent != agent.name()
        {
            return Err(Error::Identity);
        }
        if let Some(session) = &self.session
            && (session.source != SOURCE || !agent.accepts(session))
        {
            return Err(Error::Session);
        }
        if self.environment.iter().any(|(key, value)| {
            !environment_keys(agent).contains(&key.as_str())
                || value.is_empty()
                || value.len() > 4096
                || value.chars().any(char::is_control)
                || (key != "PI_CONFIG_DIR" && !absolute(value))
        }) {
            return Err(Error::Environment);
        }
        Ok(())
    }

    /// The exact selected session, with explicit Sheprd provenance. A cleared
    /// selection remains none even if an older record or argv names a session.
    pub fn selected(&self) -> Option<&AgentSession> {
        self.session.as_ref()
    }
}

fn absolute(value: &str) -> bool {
    value.starts_with('/')
        || value.as_bytes().get(..3).is_some_and(|prefix| {
            prefix[0].is_ascii_alphabetic()
                && prefix[1] == b':'
                && matches!(prefix[2], b'/' | b'\\')
        })
}

/// Configuration paths only. Credentials and unrelated inherited variables
/// never enter a binding, a remote response, or a resume command.
pub fn environment_keys(agent: AgentKind) -> &'static [&'static str] {
    match agent {
        AgentKind::Claude => &["CLAUDE_CONFIG_DIR"],
        AgentKind::Codex => &["CODEX_HOME"],
        AgentKind::Copilot => &["COPILOT_HOME"],
        AgentKind::Opencode => &[
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_DIR",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
        ],
        AgentKind::Pi => &["PI_CODING_AGENT_DIR", "PI_CONFIG_DIR"],
        AgentKind::Omp => &["PI_CODING_AGENT_DIR", "PI_CONFIG_DIR"],
        _ => &[],
    }
}

/// Construct a reference from an explicit agent event, not from discovery by
/// recency. The caller still has to bind and verify its producing process.
pub fn reference(agent: AgentKind, kind: SessionKind, value: String) -> Result<AgentSession> {
    let session = AgentSession {
        agent: agent.name().to_owned(),
        kind,
        source: SOURCE.into(),
        value,
    };
    if !agent.accepts(&session) {
        return Err(Error::Session);
    }
    Ok(session)
}

#[cfg(test)]
mod tests;
