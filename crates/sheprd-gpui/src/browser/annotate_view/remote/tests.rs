#![allow(clippy::unwrap_used)]
use super::*;

fn host() -> ConnectTarget {
    ConnectTarget::Wsl {
        distro: "test-only".into(),
        session: "default".into(),
    }
}

#[test]
fn remote_paths_keep_note_slots_and_quote_only_the_prompt_not_cleanup() {
    let expected = host();
    let local = PathBuf::from("/private/local-image.png");
    let image = Arc::new(Image::from_bytes(gpui::ImageFormat::Png, b"png".to_vec()));
    let mut uploaded = upload_with(
        &expected,
        &[None, Some(image)],
        &AtomicBool::new(false),
        |_| Ok(vec![None, Some(local.clone())]),
        |target, paths, _| {
            assert_eq!(target, &expected);
            assert_eq!(paths, std::slice::from_ref(&local));
            Ok(vec!["/remote/note one's.png".into()])
        },
    )
    .unwrap();
    assert_eq!(
        uploaded.paths,
        vec![None, Some(PathBuf::from("'/remote/note one'\\''s.png'"))]
    );
    assert_eq!(
        uploaded.cleanup,
        Some((expected, vec!["/remote/note one's.png".into()]))
    );
    // Accepted uploads are no longer owned by the in-flight guard.
    uploaded.cleanup = None;
}

#[test]
fn screenshot_budget_and_cancellation_are_checked_before_saving() {
    let image = Arc::new(Image::from_bytes(
        gpui::ImageFormat::Png,
        vec![0; MAX_SCREENSHOT_BYTES + 1],
    ));
    let result = upload_with(
        &host(),
        &[Some(image)],
        &AtomicBool::new(false),
        |_| panic!("oversized artifact must not be saved"),
        |_, _, _| panic!("oversized artifact must not be uploaded"),
    );
    assert!(matches!(
        result,
        Err(crate::Error::ImageInputTooLarge { .. })
    ));
    let result = upload_with(
        &host(),
        &[],
        &AtomicBool::new(true),
        |_| panic!("cancelled"),
        |_, _, _| panic!("cancelled"),
    );
    assert!(result.is_err());
}

#[test]
fn text_only_feedback_needs_no_remote_file_operation() {
    let uploaded = upload_with(
        &host(),
        &[None],
        &AtomicBool::new(false),
        |_| Ok(vec![None]),
        |_, _, _| panic!("text has no files"),
    )
    .unwrap();
    assert_eq!(uploaded.paths, vec![None]);
}

#[test]
fn a_failed_transfer_never_substitutes_local_paths() {
    let image = Arc::new(Image::from_bytes(gpui::ImageFormat::Png, b"png".to_vec()));
    let result = upload_with(
        &host(),
        &[Some(image)],
        &AtomicBool::new(false),
        |_| Ok(vec![Some(PathBuf::from("/private/local.png"))]),
        |_, _, _| Err(sheprd_client::Error::UploadCancelled.into()),
    );
    assert!(result.is_err());
}

#[test]
fn remote_notes_name_the_managed_shim_not_the_desktop_executable() {
    assert_eq!(
        crate::remote_browser::reload_command(),
        "~/.local/state/herdr/gpui/bin/herdr-gpui browser reload"
    );
}
