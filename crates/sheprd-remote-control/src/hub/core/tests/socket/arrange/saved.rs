use super::*;

#[test]
fn a_failed_saved_split_ends_only_the_shell_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("browser-tabs.json");
    // The tab vanished since the phone read the layout.
    std::fs::write(&path, r#"{"tabs":[]}"#).unwrap();
    let (mut source, fake) = fake();
    let editable = Arc::get_mut(&mut source).unwrap();
    editable.tabs = Some(path.clone());
    editable.scope = Some("local:/test.sock".into());
    source.poll().unwrap();
    assert!(source.arrange_saved(4, "t1", &Op::SplitRight).is_err());
    let run = run_request(&fake);
    let kill = run_request(&fake);
    assert_eq!(run["method"], "terminal.run");
    assert_eq!(kill["method"], "terminal.kill");
    assert_eq!(kill["params"]["terminal"], "t9");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"tabs":[]}"#);
    drop(source);
    fake.server.join().unwrap();
}
