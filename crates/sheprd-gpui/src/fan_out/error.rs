//! Fan-out failures keep the step they happened in and their typed cause.

/// The part of a fan-out that failed, for context in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Detect,
    StartAgent,
    Compare,
}

impl Step {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Detect => "finding installed agents", // i18n: data
            Self::StartAgent => "starting the agent",   // i18n: data
            Self::Compare => "reading the changes",     // i18n: data
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(
        "The agent may already be running, but its terminal was not saved. Keep the checkout and inspect it before starting another agent."
    )]
    AgentUncertain,
    #[error(transparent)]
    Checkout(#[from] sheprd_github::Error),
    #[error("{} failed: {source}", step.label())]
    Script {
        step: Step,
        #[source]
        source: sheprd_client::Error,
    },
    #[error("The base commit could not be read")]
    NoBase,
    #[error(transparent)]
    Dispatch(crate::teleport::Error),
    #[error("{host} could not be set up")]
    HostUnavailable { host: String },
    #[error("{agent} is not installed on {host}")]
    AgentMissing { agent: &'static str, host: String },
    #[error("Fan-out cancelled")]
    Cancelled,
    /// A lane's worktree or agent, on Herdr 1 or Herdr 2.
    #[error(transparent)]
    Work(Box<sheprd_remote::work::Error>),
    #[error("The saved fan-outs could not be read")]
    JournalRead(#[source] Box<crate::Error>),
    #[error("The saved fan-outs are not valid JSON")]
    JournalJson(#[source] serde_json::Error),
    #[error("The fan-out's progress could not be saved")]
    JournalWrite(#[source] Box<crate::Error>),
    #[error(
        "Too many fan-outs are saved; this one will not come back after a quit. Forget one to make room."
    )]
    JournalFull,
    #[error(
        "A name or path in this fan-out is too long to save; it will not come back after a quit."
    )]
    JournalFieldTooLong,
}

pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;

/// `map_err` adapter attaching the step to a script failure.
pub(crate) fn script(step: Step) -> impl FnOnce(sheprd_client::Error) -> Error {
    move |source| match source {
        sheprd_client::Error::ScriptCancelled => Error::Cancelled,
        source => Error::Script { step, source },
    }
}

#[cfg(test)]
mod tests;
