use super::*;

#[test]
fn a_headless_chat_fails_unless_its_agent_ended_as_expected() {
    assert!(headless_result(true).is_ok());
    let error = headless_result(false).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<crate::Error>(),
        Some(crate::Error::AgentEnded)
    ));
}
