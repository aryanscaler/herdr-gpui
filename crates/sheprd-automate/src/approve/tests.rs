use super::Panes;

#[test]
fn both_panes_are_kept_and_empty_ones_are_unset() {
    let both = Panes::of(Some("w1p1"), Some("t3"));
    assert_eq!((both.herdr, both.core), (Some("w1p1"), Some("t3")));
    // A terminal Sheprd starts clears HERDR_PANE_ID.
    let core = Panes::of(Some(""), Some("t3"));
    assert_eq!((core.herdr, core.core), (None, Some("t3")));
    assert!(core.any());
    assert!(!Panes::of(Some(""), Some("")).any());
    assert!(!Panes::of(None, None).any());
}

#[test]
fn a_terminal_its_core_cannot_describe_refuses_with_why() {
    let error = super::Error::CoreUnknown {
        terminal: "t3".into(),
        source: crate::herdr_core::Error::NoTerminal("t3".into()),
    };
    assert!(error.to_string().contains("Herdr 2 terminal t3"));
    let source = std::error::Error::source(&error).map(ToString::to_string);
    assert_eq!(source.as_deref(), Some("herdr-core lists no terminal t3"));
}
