use super::*;

#[test]
fn an_ambiguous_send_keeps_the_original_terminal_and_the_error() {
    let terminal = sheprd_remote::work::Started::Herdr2 {
        terminal: "t9".into(),
    };
    let error = sheprd_remote::work::Error::Unprompted {
        started: terminal.clone(),
        source: Box::new(sheprd_remote::work::Error::Cancelled),
    };
    let (started, error) = kept_start_outcome(Err(error.into()));
    assert_eq!(started, Some(terminal));
    assert!(matches!(
        error,
        Some(crate::Error::Remote(sheprd_remote::Error::Work(
            sheprd_remote::work::Error::Unprompted { .. }
        )))
    ));
    let (started, error) = kept_start_outcome(Err(sheprd_remote::work::Error::Cancelled.into()));
    assert_eq!(started, None);
    assert!(error.is_some());
}
