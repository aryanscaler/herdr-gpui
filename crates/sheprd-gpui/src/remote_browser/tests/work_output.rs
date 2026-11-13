use std::process::Command;

/// Runs the shim's `output` on `response` and returns its exit status and
/// what it printed.
fn output(response: &str) -> (i32, String) {
    let program = format!(
        "{}\nimport json\nsys.exit(output(json.loads({response:?}), False))\n",
        include_str!("../../remote_work/shim.py")
            .replace("if __name__ == \"__main__\":", "if False:")
    );
    let result = Command::new("python3")
        .args(["-I", "-c", &program])
        .output()
        .unwrap();
    (
        result.status.code().unwrap(),
        String::from_utf8(result.stdout).unwrap(),
    )
}

/// The full CLI exits 4 when there is nothing to show; so does the shim.
#[test]
fn an_empty_remote_note_listing_exits_4_like_the_full_cli() {
    let (status, text) =
        output(r#"{"status":"work","reply":{"kind":"notes","notes":[],"omitted":0}}"#);
    assert_eq!(status, 4);
    assert_eq!(text.trim(), "No worktree notes");
    let (status, _) =
        output(r#"{"status":"work","reply":{"kind":"notes","notes":[],"omitted":3}}"#);
    assert_eq!(status, 0);
    let (status, _) = output(r#"{"status":"work","reply":{"kind":"note","note":null}}"#);
    assert_eq!(status, 4);
}
