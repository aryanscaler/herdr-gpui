use super::*;

#[test]
fn remote_browser_origins_require_the_exact_host_and_a_driving_pane() {
    let terminals = terminals();
    let origin = crate::chat::core_pane::key_on(&Host::Remote(scope()), &id("t1"));
    assert_eq!(
        terminals.browser_terminal(&scope(), &origin),
        Some(id("t1"))
    );
    assert_eq!(
        terminals.browser_terminal(&Scope::endpoint("ssh:other"), &origin),
        None
    );
    let mirror = crate::chat::core_pane::key_on(&Host::Remote(scope()), &id("t2"));
    assert_eq!(terminals.browser_terminal(&scope(), &mirror), None);
    assert_eq!(terminals.browser_terminal(&scope(), "core:t1"), None);
}

#[test]
fn feedback_and_reload_keep_first_snapshot_and_unshown_request_fences() {
    for action in ["reload", "feedback-600"] {
        let mut terminals = terminals();
        let old = [("herdr-gpui-old", action)];
        terminals.list_for_test(&scope(), vec![labelled("t1", &old)]);
        assert!(terminals.label_requests().is_empty());
        let new = [("herdr-gpui-new", action)];
        terminals.list_for_test(&scope(), vec![labelled("t1", &old), labelled("t9", &new)]);
        assert!(terminals.label_requests().is_empty());
        terminals.tabs.insert(
            TabId::test(9),
            TabPanes::new(Host::Remote(scope()), Panes::single(id("t9")), false),
        );
        assert!(terminals.label_requests().is_empty());
        terminals.list_for_test(
            &scope(),
            vec![labelled("t1", &[("herdr-gpui-fresh", action)])],
        );
        let found = terminals.label_requests();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].terminal, id("t1"));
        assert!(terminals.label_requests().is_empty());
    }
}

#[test]
fn removing_the_callers_tab_cancels_its_wait_even_with_other_host_tabs_present() {
    let mut terminals = terminals();
    assert!(terminals.drives_browser_request(&scope(), &id("t1")));
    assert!(
        !terminals.drives_browser_request(&scope(), &id("t2")),
        "mirror cannot acknowledge"
    );
    assert!(!terminals.drives_browser_request(&Scope::endpoint("ssh:other"), &id("t1")));
    terminals.tabs.remove(&TabId::test(1));
    assert!(
        !terminals.tabs.is_empty(),
        "another tab still holds this host"
    );
    assert!(!terminals.drives_browser_request(&scope(), &id("t1")));
}
