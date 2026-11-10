//! Mutes of the workspaces and Herdr 2 tabs the app keeps itself.

use super::*;

const SCOPE: &str = "local:/tmp/herdr.sock";

fn all(_: &str, _: Scope, _: &str) -> bool {
    true
}

#[test]
fn a_kept_tab_mutes_and_unmutes_and_is_saved() {
    let mut mutes = Mutes::default();
    let now = SystemTime::now();
    assert!(mutes.set_kept(SCOPE, Scope::Tab, "7", true, now, all));
    assert!(mutes.kept(SCOPE, Scope::Tab, "7"));
    assert!(
        !mutes.kept(SCOPE, Scope::Workspace, "7"),
        "kinds stay apart"
    );
    assert!(!mutes.kept("other", Scope::Tab, "7"), "scopes stay apart");
    assert!(!mutes.is_empty());
    assert!(
        !mutes.set_kept(SCOPE, Scope::Tab, "7", true, now, all),
        "muting what is muted changes nothing"
    );
    assert!(mutes.reloaded().kept(SCOPE, Scope::Tab, "7"));
    let revision = mutes.revision();
    assert!(mutes.set_kept(SCOPE, Scope::Tab, "7", false, now, all));
    assert!(!mutes.kept(SCOPE, Scope::Tab, "7"));
    assert!(mutes.revision() > revision);
    assert!(mutes.is_empty());
}

#[test]
fn a_kept_mute_of_what_is_gone_is_forgotten() {
    let mut mutes = Mutes::default();
    let now = SystemTime::now();
    mutes.set_kept(SCOPE, Scope::Tab, "1", true, now, all);
    mutes.set_kept(SCOPE, Scope::Workspace, "space-1", true, now, all);
    // Tab 1 closed; muting tab 2 forgets its mute.
    let open = |_: &str, kind: Scope, id: &str| kind == Scope::Workspace || id == "2";
    assert!(mutes.set_kept(SCOPE, Scope::Tab, "2", true, now, open));
    assert!(!mutes.kept(SCOPE, Scope::Tab, "1"));
    assert!(mutes.kept(SCOPE, Scope::Tab, "2"));
    assert!(mutes.kept(SCOPE, Scope::Workspace, "space-1"));
    // Closing the workspace forgets it and the tabs that closed with it.
    assert!(mutes.forget_kept(SCOPE, "space-1", &["2".to_owned()]));
    assert!(mutes.is_empty());
    assert!(!mutes.forget_kept(SCOPE, "space-1", &[]));
}

#[test]
fn a_kept_mute_too_long_to_keep_is_refused() {
    let mut mutes = Mutes::default();
    let long = "x".repeat(MAX_FIELD_BYTES + 1);
    assert!(!mutes.set_kept(SCOPE, Scope::Tab, &long, true, SystemTime::now(), all));
    assert!(!mutes.set_kept(SCOPE, Scope::Tab, "a\nb", true, SystemTime::now(), all));
    assert!(mutes.is_empty());
}

#[test]
fn a_file_with_kept_mutes_reads_back_and_one_without_still_reads() {
    let mut mutes = Mutes::default();
    mutes.set_kept(
        SCOPE,
        Scope::Workspace,
        "space-2",
        true,
        SystemTime::now(),
        all,
    );
    let saved = serde_json::to_vec(&Saved {
        mutes: Vec::new(),
        hosts: Vec::new(),
        kept: mutes.kept.clone(),
    })
    .unwrap();
    let parsed = parse(&saved).unwrap();
    assert_eq!(parsed.kept, mutes.kept);
    let old = parse(br#"{"mutes":[],"hosts":[]}"#).unwrap();
    assert!(old.kept.is_empty());
    let damaged = br#"{"mutes":[],"kept":[{"scope":"","kind":"tab","id":"1","muted_at":0}]}"#;
    assert!(parse(damaged).is_err());
}

#[test]
fn the_largest_set_of_kept_mutes_fits_in_the_file() {
    let mut mutes = Mutes::default();
    let scope = "\"".repeat(MAX_FIELD_BYTES);
    for index in 0..crate::notification_mutes::kept::MAX_KEPT + 1 {
        let id = format!("{index:\"<MAX_FIELD_BYTES$}");
        assert!(mutes.set_kept(&scope, Scope::Tab, &id, true, SystemTime::now(), all));
    }
    assert_eq!(
        mutes.kept.len(),
        crate::notification_mutes::kept::MAX_KEPT,
        "the oldest gave way"
    );
    let bytes = serde_json::to_vec(&Saved {
        mutes: Vec::new(),
        hosts: Vec::new(),
        kept: mutes.kept.clone(),
    })
    .unwrap();
    assert!(bytes.len() as u64 <= MAX_FILE_BYTES);
    assert_eq!(
        parse(&bytes).unwrap().kept.len(),
        crate::notification_mutes::kept::MAX_KEPT
    );
}

#[test]
fn a_closed_tabs_mute_is_forgotten_at_launch() {
    let mut mutes = Mutes::default();
    let now = SystemTime::now();
    mutes.set_kept(SCOPE, Scope::Tab, "3", true, now, all);
    mutes.set_kept(SCOPE, Scope::Tab, "4", true, now, all);
    mutes.set_kept(SCOPE, Scope::Workspace, "space-1", true, now, all);
    // Tab 4 closed before the app quit; its ID may come back on a new tab.
    assert!(mutes.forget_closed_tabs(|_, id| id == "3"));
    assert!(mutes.kept(SCOPE, Scope::Tab, "3"));
    assert!(!mutes.kept(SCOPE, Scope::Tab, "4"));
    assert!(
        mutes.kept(SCOPE, Scope::Workspace, "space-1"),
        "workspace IDs are never handed out twice"
    );
    assert!(!mutes.forget_closed_tabs(|_, _| true));
}
