use super::*;

fn started() -> TerminalInfo {
    TerminalInfo {
        id: TerminalId::new("owned-new-terminal").unwrap(),
        agent: sheprd_client::protocol::AgentStatus::Unknown,
        cwd: None,
        foreground: None,
        labels: Default::default(),
        name: None,
        reporter: None,
        state: sheprd_backend::RunState::Running,
        title: None,
    }
}

#[test]
fn cancelling_a_start_ends_only_the_terminal_returned_by_its_original_core() {
    let (send, receive) = mpsc::sync_channel(1);
    drop(receive);
    let mut ended = None;
    deliver(Ok(started()), send, |terminal| {
        ended = Some(terminal.clone())
    });
    assert_eq!(ended, Some(started().id));
}

#[test]
fn an_accepted_start_stays_running_and_a_failed_start_never_kills_anything() {
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        scope.spawn(|| deliver(Ok(started()), send, |_| panic!("accepted terminal killed")));
        assert_eq!(
            receive
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .take()
                .unwrap(),
            started()
        );
    });
    let (send, receive) = mpsc::sync_channel(1);
    drop(receive);
    deliver(Err(sheprd_core_client::Error::Busy), send, |_| {
        panic!("failure killed a terminal")
    });
}

#[test]
fn dropping_a_buffered_unclaimed_result_also_ends_the_created_terminal() {
    let (send, receive) = mpsc::sync_channel(1);
    let mut ended = None;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            deliver(Ok(started()), send, |terminal| {
                ended = Some(terminal.clone())
            })
        });
        let buffered = receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        drop(buffered);
    });
    assert_eq!(ended, Some(started().id));
}
