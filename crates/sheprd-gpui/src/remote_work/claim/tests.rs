use super::*;

const KEY: &str = "sheprd-work-0123456789abcdef0123456789abcdef";
const CLAIM: &str = "sheprd-work-claim-0123456789abcdef0123456789abcdef";

fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn only_an_unclaimed_request_as_read_is_open() {
    assert!(open(&labels(&[(KEY, "req")]), KEY, CLAIM, "req"));
    assert!(!open(&labels(&[]), KEY, CLAIM, "req"));
    assert!(!open(&labels(&[(KEY, "other")]), KEY, CLAIM, "req"));
    assert!(!open(
        &labels(&[(KEY, "req"), (CLAIM, CLAIMED)]),
        KEY,
        CLAIM,
        "req"
    ));
}

#[test]
fn a_request_withdrawn_before_the_marker_is_not_held() {
    // The caller cancelled between the read and the marker: the core's
    // answer to the marker lacks the request, so the claim is abandoned.
    assert!(!held(&labels(&[(CLAIM, CLAIMED)]), KEY, CLAIM, "req"));
    assert!(held(
        &labels(&[(KEY, "req"), (CLAIM, CLAIMED)]),
        KEY,
        CLAIM,
        "req"
    ));
    assert!(!held(
        &labels(&[(KEY, "other"), (CLAIM, CLAIMED)]),
        KEY,
        CLAIM,
        "req"
    ));
}

#[test]
fn a_request_another_desktop_claimed_is_not_claimed_again() {
    let first = with_claim(&serde_json::Value::Null, KEY).unwrap();
    assert_eq!(first, serde_json::json!([KEY]));
    assert_eq!(with_claim(&first, KEY), None);
    let other = "sheprd-work-ffffffffffffffffffffffffffffffff";
    assert_eq!(
        with_claim(&first, other).unwrap(),
        serde_json::json!([KEY, other])
    );
}

#[test]
fn the_claim_document_keeps_only_the_most_recent_requests() {
    let mut value = serde_json::Value::Null;
    for index in 0..RECENT + 5 {
        value = with_claim(&value, &format!("sheprd-work-{index:032x}")).unwrap();
    }
    let keys = value.as_array().unwrap();
    assert_eq!(keys.len(), RECENT);
    assert_eq!(keys[0], format!("sheprd-work-{:032x}", 5));
    assert_eq!(
        with_claim(&value, &format!("sheprd-work-{:032x}", RECENT + 4)),
        None
    );
}
