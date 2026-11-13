use super::*;

/// A chat recovered while Sheprd was closed: the phone shows its replacement
/// `t9`, while the saved file still names the terminal `t1` it replaced.
#[test]
fn a_phone_changes_a_recovered_pane_the_saved_file_still_names_by_its_old_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = saved_file(dir.path());
    let source = tests_support_source(Some(path.clone()), Some(SCOPE));
    lock(&source.poller).replacements = BTreeMap::from([(
        TerminalId::new("t1").unwrap(),
        TerminalId::new("t9").unwrap(),
    )]);
    source
        .arrange_saved(4, "t9", &Op::RenamePane(Some("server".into())))
        .unwrap();
    source.arrange_saved(4, "t9", &Op::Zoom).unwrap();
    let location = &read(&path)["tabs"][0]["location"];
    assert_eq!(location["panes"]["first"], "t9");
    assert_eq!(location["panes"]["second"], "t2");
    assert_eq!(location["names"], json!({"t9": "server"}));
    assert_eq!(location["zoomed"], "t9");
}
