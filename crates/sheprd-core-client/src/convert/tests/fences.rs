use super::*;

fn running() -> Terminal {
    serde_json::from_value(serde_json::json!({
        "address": "t1", "state": "live", "pid": 100,
        "status": {"state": "idle", "app": "claude"},
        "foreground": {"pid": 200, "name": "claude", "argv": ["claude"]}
    }))
    .unwrap()
}

#[test]
fn another_process_of_the_same_kind_is_not_interrupted() {
    let before = running();
    assert!(unchanged(&before, &before, Agent::Claude).is_ok());
    let mut now = before.clone();
    now.foreground.as_mut().unwrap().pid = 201;
    assert!(matches!(
        unchanged(&before, &now, Agent::Claude),
        Err(crate::Error::Convert(convert::Error::Moved))
    ));
}

#[test]
fn an_agent_that_started_work_during_discovery_is_not_interrupted() {
    let before = running();
    let mut now = before.clone();
    now.status.as_mut().unwrap().state = crate::wire::StatusState::Working;
    assert!(matches!(
        unchanged(&before, &now, Agent::Claude),
        Err(crate::Error::Convert(convert::Error::Busy(Agent::Claude)))
    ));
}

#[test]
fn a_replacement_shell_never_receives_the_resume_line() {
    let before = running();
    let mut now = before.clone();
    let front = now.foreground.as_mut().unwrap();
    front.pid = 100;
    front.name = "zsh".into();
    front.argv = vec!["zsh".into()];
    assert!(returned(&before, &now, Agent::Claude));
    now.pid = Some(101);
    now.foreground.as_mut().unwrap().pid = 101;
    assert!(!returned(&before, &now, Agent::Claude));
}
