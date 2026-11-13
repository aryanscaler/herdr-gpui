use super::*;

fn receipt() -> Result<Receipt, Box<dyn std::error::Error>> {
    Ok(Receipt {
        version: 1,
        scope: "remote-host".into(),
        endpoint_id: "remote-host".into(),
        workspace: Landed::New {
            checkout: "/work/feature".into(),
            label: Some("Feature".into()),
        },
        repo_key: "/work/repo/.git".into(),
        branch: "feature".into(),
        core_tabs: vec![Panes::single(TerminalId::new("t5")?)],
    })
}

#[test]
fn receipt_survives_until_the_correct_host_tabs_are_saved() -> Result<(), Box<dyn std::error::Error>>
{
    let home = tempfile::tempdir()?;
    let pending = home.path().join("pending");
    let saved = home.path().join("browser-tabs.json");
    let receipt = receipt()?;
    let path = save(&pending, &receipt)?;
    assert!(!release(&path, &receipt, &saved, &[])?);
    let loaded = load(&pending)?;
    assert_eq!(loaded.len(), 1);
    let recovered = loaded[0].1.outcome();
    assert!(
        !recovered.source_retired,
        "recovery must not guess source ownership"
    );
    assert_eq!(recovered.workspace, receipt.workspace);
    assert_eq!(recovered.core_tabs, receipt.core_tabs);
    let tabs = |scope| serde_json::json!({"tabs":[{"id":1,"scope":scope,"workspace_id":"space-1","title":"Feature","location":{"kind":"core_terminal","panes":"t5"}}]});
    fs::write(&saved, serde_json::to_vec(&tabs("other-host"))?)?;
    assert!(!release(&path, &receipt, &saved, &[])?);
    assert!(path.exists());
    fs::write(&saved, serde_json::to_vec(&tabs("remote-host"))?)?;
    assert!(release(&path, &receipt, &saved, &[])?);
    assert!(load(&pending)?.is_empty());
    Ok(())
}

#[test]
fn invalid_and_oversize_receipts_do_not_allocate_unboundedly()
-> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let mut record = receipt()?;
    record.version = 2;
    let path = save(home.path(), &record)?;
    assert!(matches!(read(&path), Err(Error::Version)));
    fs::write(&path, vec![b' '; MAX_BYTES as usize + 1])?;
    assert!(matches!(read(&path), Err(Error::TooLarge)));
    save(home.path(), &receipt()?)?;
    assert_eq!(
        load(home.path())?.len(),
        1,
        "a corrupt receipt must not block other recoveries"
    );
    Ok(())
}

#[test]
fn closed_tabs_are_not_waited_for() -> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let saved = home.path().join("browser-tabs.json");
    let mut receipt = receipt()?;
    receipt
        .core_tabs
        .push(Panes::single(TerminalId::new("t6")?));
    let path = save(&home.path().join("pending"), &receipt)?;
    // The user kept t5 and closed t6; the saved file holds only t5.
    let tabs = serde_json::json!({"tabs":[{"id":1,"scope":"remote-host","workspace_id":"space-1","title":"Feature","location":{"kind":"core_terminal","panes":"t5"}}]});
    fs::write(&saved, serde_json::to_vec(&tabs)?)?;
    assert!(!release(&path, &receipt, &saved, &[])?);
    let closed = [TerminalId::new("t6")?];
    assert!(release(&path, &receipt, &saved, &closed)?);
    assert!(!path.exists());
    Ok(())
}
