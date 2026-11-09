use super::*;

#[test]
fn history_answers_come_back_by_id_and_a_failure_keeps_the_terminal() {
    let backend = Scripted::default();
    let link = worker::spawn(
        backend.clone(),
        TerminalId::new("t1").unwrap(),
        size(80, 24),
        Role::Active,
    )
    .unwrap();
    link.send(Command::History {
        id: 7,
        request: History::Clear,
    });
    link.send(Command::History {
        id: 8,
        request: History::ScrollTo { offset: 3 },
    });
    let mut answers = Vec::new();
    until(|| {
        answers.extend(link.mailbox.take_answers());
        (answers.len() == 2).then_some(())
    });
    assert_eq!(answers[0].0, 7);
    assert_eq!(
        answers[0].1.as_ref().unwrap_err().to_string(),
        "scripted failure"
    );
    assert_eq!(answers[1].0, 8);
    assert_eq!(answers[1].1.as_ref().ok(), Some(&HistoryAnswer::Done));
    assert!(link.mailbox.changed(), "an answer wakes the view");
    // The terminal still takes commands after the failed one.
    link.send(Command::Focus(true));
    until(|| {
        backend
            .received
            .lock()
            .unwrap()
            .contains(&Command::Focus(true))
            .then_some(())
    });
}

#[test]
fn a_resizes_screen_is_shown_before_a_later_request_is_answered() {
    let backend = Scripted::default();
    let link = worker::spawn(
        backend.clone(),
        TerminalId::new("t1").unwrap(),
        size(80, 24),
        Role::Active,
    )
    .unwrap();
    until(|| (!backend.received.lock().unwrap().is_empty()).then_some(()));
    *backend.on_resize.lock().unwrap() = Some(vec![SurfaceUpdate::Title("rewrapped".into())]);
    // Held in `next`, the worker takes both commands in one go.
    let gate = backend.gate.lock().unwrap();
    link.send(Command::Resize(size(100, 30)));
    link.send(Command::History {
        id: 1,
        request: History::Text,
    });
    drop(gate);
    until(|| (!link.mailbox.take_answers().is_empty()).then_some(()));
    let received = backend.received.lock().unwrap().clone();
    let resized = received
        .iter()
        .position(|command| *command == Command::Resize(size(100, 30)))
        .unwrap();
    assert!(matches!(received[resized + 1], Command::History { .. }));
    // The resized screen went out after the resize and before the read.
    assert_eq!(*backend.handed.lock().unwrap(), [resized + 1]);
}
