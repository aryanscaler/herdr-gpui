//! The mailbox hands the view each answer with a screen at least as new as
//! the one the worker showed before making it.

use super::super::super::worker::Mailbox;
use sheprd_backend::HistoryAnswer;
use std::sync::Arc;

#[test]
fn an_answer_never_comes_with_an_older_screen_than_it_was_made_from() {
    let mailbox = Arc::new(Mailbox::default());
    let rounds = 20_000u64;
    let worker = {
        let mailbox = Arc::clone(&mailbox);
        std::thread::spawn(move || {
            // As the worker does: each screen first, then what it answers.
            for revision in 1..=rounds {
                mailbox.update(|screen| screen.revision = revision);
                mailbox.answer(revision, Ok(HistoryAnswer::Done));
            }
        })
    };
    let mut seen = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while seen < rounds {
        assert!(
            std::time::Instant::now() < deadline,
            "only answers up to {seen} arrived"
        );
        let (screen, answers) = mailbox.take_with_answers();
        let Some(screen) = screen else {
            std::hint::spin_loop();
            continue;
        };
        for (made_at, _) in answers {
            assert!(
                made_at <= screen.revision,
                "answer from revision {made_at} came with screen {}",
                screen.revision
            );
            seen = made_at;
        }
    }
    worker.join().unwrap();
}

#[test]
fn bells_and_clipboard_writes_are_taken_with_the_answers_once() {
    let mailbox = Mailbox::default();
    mailbox.update(|screen| {
        screen.bells = 2;
        screen.clipboard = Some("copied".into());
    });
    mailbox.answer(1, Ok(HistoryAnswer::Done));
    let (screen, answers) = mailbox.take_with_answers();
    let screen = screen.unwrap();
    assert_eq!(
        (screen.bells, screen.clipboard.as_deref()),
        (2, Some("copied"))
    );
    assert_eq!(answers.len(), 1);
    // A later screen does not ring or copy them again.
    mailbox.update(|screen| screen.revision = 1);
    let screen = mailbox.take_with_answers().0.unwrap();
    assert_eq!((screen.bells, screen.clipboard), (0, None));
}
