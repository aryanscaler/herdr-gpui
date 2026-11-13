//! A restore must not resume a session while the agent it was last given,
//! or a tool in that agent's process group, may still be at work.
use super::*;
use std::ffi::OsString;

fn spawned(script: &str) -> crate::process::Spawned {
    let dir = tempfile::tempdir().unwrap();
    crate::process::spawn(
        &OsString::from("/bin/sh"),
        &["-c".into(), script.into()],
        &[],
        dir.path(),
    )
    .unwrap()
}

#[test]
fn an_unknown_previous_agent_refuses_the_restore() {
    let dir = tempfile::tempdir().unwrap();
    assert!(previous_agent_gone(dir.path(), &ID.parse().unwrap()).is_err());
}

#[test]
fn a_live_agent_is_not_gone_and_an_ended_one_is() {
    let dir = tempfile::tempdir().unwrap();
    let id: EndpointId = ID.parse().unwrap();
    let agent = spawned("exec sleep 30");
    agent_started(dir.path(), &id, agent.process.id()).unwrap();
    assert!(!previous_agent_gone(dir.path(), &id).unwrap());
    agent.process.stop();
    assert!(previous_agent_gone(dir.path(), &id).unwrap());
    agent_ended(dir.path(), &id);
    assert!(previous_agent_gone(dir.path(), &id).is_err());
}

#[test]
fn a_tool_left_in_the_agents_group_keeps_the_agent_from_being_gone() {
    let id: EndpointId = ID.parse().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // The agent exits at once; the tool it started lives on in its group.
    let agent = spawned("sleep 30 & echo $! ; exit 0");
    let pid = agent.process.id();
    agent_started(dir.path(), &id, pid).unwrap();
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(agent.output), &mut line).unwrap();
    agent.process.wait().unwrap();
    assert!(!previous_agent_gone(dir.path(), &id).unwrap());
    let tool = rustix::process::Pid::from_raw(line.trim().parse().unwrap()).unwrap();
    rustix::process::kill_process(tool, rustix::process::Signal::KILL).unwrap();
    let group = rustix::process::Pid::from_raw(i32::try_from(pid).unwrap()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    // Until the group's last member is reaped, macOS may answer EPERM rather
    // than ESRCH, which the restore rightly reads as not gone.
    while rustix::process::test_kill_process_group(group) != Err(rustix::io::Errno::SRCH) {
        assert!(std::time::Instant::now() < deadline, "the tool did not end");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(previous_agent_gone(dir.path(), &id).unwrap());
}
