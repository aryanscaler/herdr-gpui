use super::*;

#[test]
fn a_core_agent_cannot_supply_an_inferred_herdr_1_runs_status() {
    let issue = item(&github(), "1", "Working");
    let workspaces = [workspace("", &branch(&issue))];
    let agents = [LiveAgent {
        terminals: Terminals::Herdr2,
        ..agent("", AgentStatus::Blocked)
    }];
    let found = found_runs(&[issue], &[], &workspaces, &[], &agents);
    assert_eq!(found[0].state, RunState::Cancelled);
}
