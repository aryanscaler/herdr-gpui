use super::*;

#[test]
fn an_unacknowledged_owned_launch_keeps_the_claim_while_its_agent_may_work() {
    let dir = tempfile::tempdir().unwrap();
    let (site, mut request) = bead_site(dir.path(), "", UNCLAIMED);
    request.terminals = Terminals::Herdr2;
    herdr2::fake_core(dir.path(), "idle");
    std::fs::write(dir.path().join("home/lose-run-reply"), "").unwrap();
    assert!(matches!(
        dispatch(&site, request),
        Err(Error::Retained { .. })
    ));
    assert!(
        !bd_calls(dir.path())
            .iter()
            .any(|call| call.contains("unclaim"))
    );
    let store = Store::open(&site.database).unwrap();
    assert!(store.runs().unwrap()[0].workspace.is_some());
    assert_eq!(store.sessions().unwrap()[0].pane, Pane::Pending);
}
