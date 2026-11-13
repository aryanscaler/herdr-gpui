//! Sheprd's relay list: the file sheprd.org serves, empty or not, and what
//! a client refuses.

use crate::directory::{Directory, Error, MAX_BYTES, MAX_RELAYS};

/// The list as sheprd.org publishes it.
const PUBLISHED: &str = include_str!("../../../../website/site/relays.json");

#[test]
fn the_published_list_parses() {
    assert!(Directory::parse(PUBLISHED.as_bytes()).is_ok());
}

#[test]
fn an_empty_list_means_none_are_available() {
    let directory = Directory::parse(br#"{"version":1,"relays":[]}"#).unwrap();
    assert!(directory.is_empty());
}

#[test]
fn relays_parse_with_optional_labels_and_unknown_fields() {
    let directory = Directory::parse(
        br#"{
          "version": 1,
          "updated": "2026-11-11",
          "relays": [
            { "endpoint": "https://fra1.relay.sheprd.org", "region": "fra1", "name": "Frankfurt", "weight": 2 },
            { "endpoint": "nyc1.relay.sheprd.org" },
            { "endpoint": "https://FRA1.relay.sheprd.org/", "name": "duplicate" }
          ]
        }"#,
    )
    .unwrap();
    assert!(!directory.is_empty());
    assert_eq!(directory.relays.len(), 2);
    let frankfurt = &directory.relays[0];
    assert_eq!(
        frankfurt.endpoint.to_string(),
        "https://fra1.relay.sheprd.org"
    );
    assert_eq!(frankfurt.region.as_deref(), Some("fra1"));
    assert_eq!(frankfurt.name.as_deref(), Some("Frankfurt"));
    assert_eq!(directory.relays[1].name, None);
}

#[test]
fn bad_lists_are_refused_with_a_reason() {
    let bad = |json: &str| Directory::parse(json.as_bytes()).unwrap_err();
    assert!(matches!(bad("not json"), Error::Format));
    assert!(matches!(bad(r#"{"relays":[]}"#), Error::Format));
    assert!(matches!(
        bad(r#"{"version":2,"relays":[]}"#),
        Error::Version { version: 2 }
    ));
    assert!(matches!(
        bad(r#"{"version":1,"relays":[{"endpoint":"http://192.168.1.2"}]}"#),
        Error::Insecure { index: 0 }
    ));
    assert!(matches!(
        bad(r#"{"version":1,"relays":[{"endpoint":"ftp://relay.example"}]}"#),
        Error::Endpoint { index: 0, .. }
    ));
    let long_name = format!(
        r#"{{"version":1,"relays":[{{"endpoint":"https://a.example","name":"{}"}}]}}"#,
        "x".repeat(65)
    );
    assert!(matches!(bad(&long_name), Error::Label { field: "name" }));
    let too_many = format!(
        r#"{{"version":1,"relays":[{}]}}"#,
        (0..=MAX_RELAYS)
            .map(|index| format!(r#"{{"endpoint":"https://r{index}.example"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(matches!(bad(&too_many), Error::TooMany));
    assert!(matches!(
        Directory::parse(&vec![b' '; MAX_BYTES + 1]).unwrap_err(),
        Error::TooLarge
    ));
}
