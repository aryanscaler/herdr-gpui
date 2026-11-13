use super::*;
use crate::session::{ConnId, Input};

#[test]
fn only_user_shutdown_and_successful_agent_exit_are_expected_closure() {
    assert!(expected_end(&Input::Shutdown));
    assert!(expected_end(&Input::AgentExited {
        code: Some(0),
        stderr: Vec::new()
    }));
    for code in [None, Some(1), Some(137)] {
        assert!(!expected_end(&Input::AgentExited {
            code,
            stderr: Vec::new()
        }));
    }
    assert!(!expected_end(&Input::Detach(ConnId(1))));
}
