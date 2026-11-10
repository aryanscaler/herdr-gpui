use super::*;

#[test]
fn a_tab_moves_to_another_workspace_after_its_tabs() {
    let mut store = Store::default();
    let scope = Scope::local("/tmp/herdr-client.sock".as_ref());
    let a = store.open(scope.clone(), "w1", None, None).unwrap();
    let b = store.open(scope.clone(), "w2", None, None).unwrap();
    assert!(store.move_to_workspace(a, "w2"));
    let ids: Vec<TabId> = store.in_workspace(&scope, "w2").map(|tab| tab.id).collect();
    assert_eq!(ids, [b, a]);
    assert_eq!(store.in_workspace(&scope, "w1").count(), 0);
    assert!(!store.move_to_workspace(a, "w2"), "already there");
    assert!(!store.move_to_workspace(a, ""), "no workspace");
    assert!(!store.move_to_workspace(TabId(99), "w1"), "no such tab");
}
