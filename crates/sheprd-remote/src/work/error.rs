use std::fmt;

/// What a host script was doing when it failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    // keep-sorted start
    CreateWorktree,
    List,
    Prompt,
    Remove,
    StartAgent,
    // keep-sorted end
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CreateWorktree => "creating the worktree",
            Self::List => "listing the repository's worktrees",
            Self::Prompt => "sending the agent its prompt",
            Self::Remove => "removing the worktree",
            Self::StartAgent => "starting the agent",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    /// The agent's terminal answered as herdr-core's own agent steps do,
    /// such as an agent that is not ready for its prompt.
    #[error(transparent)]
    Agent(#[from] sheprd_core_client::Error),
    #[error("The work was cancelled")]
    Cancelled,
    #[error("{step} failed: unexpected output")]
    Decode {
        step: Step,
        #[source]
        source: serde_json::Error,
    },
    /// Removing it without force would lose changes; nothing was removed.
    #[error("The worktree has uncommitted changes")]
    Dirty,
    #[error("{step} failed: unexpected output")]
    Output { step: Step },
    #[error("{step} failed: {source}")]
    Script {
        step: Step,
        #[source]
        source: sheprd_client::Error,
    },
    /// The agent started, at `started`, but did not take its prompt.
    #[error("The agent started but did not take its prompt: {source}")]
    Unprompted {
        started: super::Started,
        #[source]
        source: Box<Error>,
    },
    #[error(transparent)]
    Worktree(#[from] sheprd_client::herdr::worktree::Error),
    // keep-sorted end
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// `map_err` adapter naming the step a script failed at.
pub(super) fn script(step: Step) -> impl FnOnce(sheprd_client::Error) -> Error {
    move |source| match source {
        sheprd_client::Error::ScriptCancelled => Error::Cancelled,
        source => Error::Script { step, source },
    }
}
