//! Fan-out failures: `error-fan-out.ftl`.

use super::Localize;
use crate::{
    fan_out::error::{Error, Step},
    i18n::fl,
};

/// The selector key of the step a script failed in.
fn step(step: Step) -> &'static str {
    match step {
        Step::Detect => "detect",
        Step::StartAgent => "start-agent",
        Step::Compare => "compare",
    }
}

impl Localize for Error {
    fn localize(&self) -> String {
        match self {
            // keep-sorted start
            Self::AgentMissing { agent, host } => fl!(
                "error-fan-out-agent-missing",
                agent = *agent,
                host = host.as_str()
            ),
            Self::AgentUncertain => fl!("error-fan-out-agent-uncertain"),
            Self::Cancelled => fl!("error-fan-out-cancelled"),
            Self::Checkout(error) => error.localize(),
            Self::Dispatch(error) => error.localize(),
            Self::HostUnavailable { host } => {
                fl!("error-fan-out-host-unavailable", host = host.as_str())
            }
            Self::JournalFieldTooLong => fl!("error-fan-out-journal-field-too-long"),
            Self::JournalFull => fl!("error-fan-out-journal-full"),
            Self::JournalJson(_) => fl!("error-fan-out-journal-json"),
            Self::JournalRead(_) => fl!("error-fan-out-journal-read"),
            Self::JournalWrite(_) => fl!("error-fan-out-journal-write"),
            Self::NoBase => fl!("error-fan-out-no-base"),
            Self::Script {
                step: failed,
                source,
            } => fl!(
                "error-fan-out-script",
                step = step(*failed),
                error = source.localize()
            ),
            Self::Work(error) => error.localize(),
            // keep-sorted end
        }
    }
}
