//! `sheprd_remote::work::Error`: a new worktree and the agent started in
//! it, on Herdr 1 or Herdr 2, from `error-remote-work.ftl`.

use crate::i18n::{errors::Localize, fl};
use sheprd_remote::work::{Error, Step};

/// The step as a selector key: `error-remote-work-script` and `-output`
/// write each step's sentence in full.
fn step(step: Step) -> &'static str {
    match step {
        // keep-sorted start
        Step::CreateWorktree => "create-worktree",
        Step::List => "list",
        Step::Prompt => "prompt",
        Step::Remove => "remove",
        Step::StartAgent => "start-agent",
        // keep-sorted end
    }
}

impl Localize for Error {
    fn localize(&self) -> String {
        match self {
            // keep-sorted start
            // herdr-core's agent steps keep their own words.
            Self::Agent(error) => fl!("error-remote-work-agent", error = error.to_string()),
            Self::Cancelled => fl!("error-remote-work-cancelled"),
            Self::Decode { step: at, .. } | Self::Output { step: at } => {
                fl!("error-remote-work-output", step = step(*at))
            }
            Self::Dirty => fl!("error-remote-work-dirty"),
            Self::Script { step: at, source } => fl!(
                "error-remote-work-script",
                step = step(*at),
                error = source.localize()
            ),
            Self::Unprompted { source, .. } => {
                fl!("error-remote-work-unprompted", error = source.localize())
            }
            Self::Worktree(error) => error.localize(),
            // keep-sorted end
        }
    }
}
