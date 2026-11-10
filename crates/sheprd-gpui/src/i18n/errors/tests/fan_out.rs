use super::*;
use crate::fan_out::error::{Error, Step};

fn decode() -> serde_json::Error {
    serde_json::from_str::<u8>("x").unwrap_err()
}

#[test]
fn every_fan_out_variant_has_a_message() {
    let mut errors: Vec<Error> = [Step::Detect, Step::StartAgent, Step::Compare]
        .into_iter()
        .map(|step| Error::Script {
            step,
            source: sheprd_client::Error::ScriptCancelled,
        })
        .collect();
    errors.extend([
        Error::NoBase,
        Error::Dispatch(crate::teleport::Error::Cancelled),
        Error::HostUnavailable {
            host: "build-box".into(),
        },
        Error::AgentMissing {
            agent: "codex",
            host: "build-box".into(),
        },
        Error::Cancelled,
        Error::JournalRead(Box::new(crate::Error::StaleConnection)),
        Error::JournalJson(decode()),
        Error::JournalWrite(Box::new(crate::Error::StaleConnection)),
        Error::JournalFull,
        Error::JournalFieldTooLong,
        Error::Work(Box::new(sheprd_remote::work::Error::Dirty)),
    ]);
    assert_localized(&errors);
}
