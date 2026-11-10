use super::*;

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .arg("-c")
        .arg(format!(
            "core.hooksPath={}",
            dir.join("unused-hooks").display()
        ))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[gpui::test]
fn deleting_a_kept_subfolder_reads_and_runs_the_root_archive(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    let lane = dir.path().join("lane");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-b", "main"]);
    git(&main, &["commit", "--allow-empty", "-m", "initial"]);
    git(
        &main,
        &["worktree", "add", "-b", "feature", lane.to_str().unwrap()],
    );
    let folder = lane.join("src");
    std::fs::create_dir(&folder).unwrap();
    write_scripts(&lane, "[scripts]\narchive = 'make clean'\n");
    let (view, cx, id) = window(cx, folder.to_str().unwrap(), Some(true));
    let _ = actions(&view, cx, &id);
    cx.update(|_, cx| view.update(cx, |view, cx| view.open_kept_deletion(cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            let deletion = view.menu.deletion.as_ref().unwrap();
            assert!(deletion.ready());
            assert!(deletion.archive.script().is_some());
            assert_eq!(
                std::fs::canonicalize(deletion.path.as_ref().unwrap()).unwrap(),
                std::fs::canonicalize(&lane).unwrap()
            );
            assert_eq!(
                std::fs::canonicalize(deletion.root.as_ref().unwrap()).unwrap(),
                std::fs::canonicalize(&main).unwrap()
            );
            let launch = view
                .kept_script_launch(ScriptKind::Archive, id, deletion.path.clone())
                .unwrap();
            assert_eq!(
                launch.checkout.unwrap().path,
                *deletion.path.as_ref().unwrap()
            );
        })
    });
}
