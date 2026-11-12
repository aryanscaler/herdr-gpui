use super::*;

#[test]
fn legacy_adoption_is_explicit_and_revalidates_the_displayed_process() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let cancelled = AtomicBool::new(false);
    perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &cancelled,
    )
    .unwrap();
    let mut store = Store::open(&site.database).unwrap();
    let run = store.runs().unwrap().remove(0);
    let mut session = store.sessions().unwrap().remove(0);
    session.target = None;
    session.checkout_token = None;
    store.update_run(&run, Some(&session)).unwrap();
    let inventory = dir.path().join("home/core-inventory");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&inventory).unwrap()).unwrap();
    value["terminals"][0]["argv"] = serde_json::json!(["sh"]);
    value["terminals"][0]["labels"] = serde_json::json!({});
    std::fs::write(&inventory, serde_json::to_vec(&value).unwrap()).unwrap();
    let before = core_calls(dir.path()).len();
    let receipt = crate::actions::legacy::proposal(&site.host, &run, &session, &cancelled).unwrap();
    assert!(
        core_calls(dir.path())[before..]
            .iter()
            .all(|call| call.starts_with("ls "))
    );
    assert!(store.sessions().unwrap()[0].adopted.is_none());
    // Cancellation at this point writes no authority and cannot send text.
    assert!(
        perform(
            &site,
            &Action::Send {
                run: run.id.clone(),
                text: "do not send".into()
            },
            &cancelled
        )
        .is_err()
    );

    value["terminals"][0]["foreground"]["pid"] = 10.into();
    std::fs::write(&inventory, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        perform(
            &site,
            &Action::Adopt {
                run: run.id.clone(),
                receipt: Box::new(receipt)
            },
            &cancelled
        )
        .is_err()
    );
    assert!(store.sessions().unwrap()[0].adopted.is_none());

    let receipt = crate::actions::legacy::proposal(&site.host, &run, &session, &cancelled).unwrap();
    perform(
        &site,
        &Action::Adopt {
            run: run.id.clone(),
            receipt: Box::new(receipt),
        },
        &cancelled,
    )
    .unwrap();
    assert!(store.sessions().unwrap()[0].adopted.is_some());
    drop(store);
    perform(
        &site,
        &Action::Send {
            run: run.id.clone(),
            text: "explicitly adopted".into(),
        },
        &cancelled,
    )
    .unwrap();
    perform(
        &site,
        &Action::Stop {
            run: run.id.clone(),
        },
        &cancelled,
    )
    .unwrap();
    perform(&site, &Action::Remove { run: run.id }, &cancelled).unwrap();
    assert!(!checkout(dir.path()).exists());
}
