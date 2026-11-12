use super::*;

#[test]
fn bitmap_is_staged_privately_with_its_format_and_bytes() {
    let bytes = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes.clone());
    let mut prepared = image_for(
        &ConnectTarget::Local,
        Source::Clipboard(image),
        &AtomicBool::new(false),
    )
    .unwrap();
    let (target, paths) = prepared.cleanup.take().unwrap();
    assert_eq!(target, ConnectTarget::Local);
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with(".png"));
    assert_eq!(std::fs::read(&paths[0]).unwrap(), bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&paths[0]).unwrap().permissions().mode() & 0o077,
            0
        );
    }
    cleanup(&target, &paths).unwrap();
    assert!(!std::path::Path::new(&paths[0]).exists());
}

#[test]
fn screenshot_thumbnail_survives_source_removal() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory
        .path()
        .join("TemporaryItems/NSIRD_Screenshot/image.png");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, b"opaque image").unwrap();
    let mut prepared = prepare(
        &ConnectTarget::Local,
        Request::Paths(vec![source.clone()]),
        &AtomicBool::new(false),
    )
    .unwrap();
    std::fs::remove_file(&source).unwrap();
    let (target, paths) = prepared.cleanup.take().unwrap();
    assert_eq!(std::fs::read(&paths[0]).unwrap(), b"opaque image");
    assert!(!prepared.text.contains("NSIRD"));
    cleanup(&target, &paths).unwrap();
}

#[test]
fn cancellation_and_path_validation_happen_before_remote_io() {
    let target = ConnectTarget::Ssh {
        target: "must-not-connect.invalid".into(),
        session: "test".into(),
    };
    assert!(matches!(
        prepare(
            &target,
            Request::Paths(vec!["/file".into()]),
            &AtomicBool::new(true)
        ),
        Err(crate::Error::Client(sheprd_client::Error::UploadCancelled))
    ));
    assert!(
        prepare(
            &target,
            Request::Paths(vec!["/control\nname".into()]),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(
        upload(
            &target,
            &vec![PathBuf::from("/file"); 257],
            &AtomicBool::new(false)
        )
        .is_err()
    );
}

#[test]
fn a_local_file_is_quoted_without_copying_or_opening_it() {
    let prepared = prepare(
        &ConnectTarget::Local,
        Request::Paths(vec!["/missing/a b'$(cmd).png".into()]),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        prepared.text,
        quote_paths(&["/missing/a b'$(cmd).png".into()]).unwrap()
    );
    assert!(prepared.cleanup.is_none());
}
