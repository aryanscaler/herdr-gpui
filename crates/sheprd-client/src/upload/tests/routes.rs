use super::*;

#[test]
fn local_route_roundtrip_and_explicit_cleanup() {
    let temp = Temp::new();
    let bytes = b"\0binary\xff\n";
    let source = temp.file("-quoted'$(no); image.png", bytes);
    let paths = upload_files_with(
        &[source],
        &AtomicBool::new(false),
        |_, _| {},
        |script| Ok(temp.remote(script)),
    )
    .unwrap();
    assert_eq!(fs::read(&paths[0]).unwrap(), bytes);
    remove_uploaded_files_with(&paths, |script| Ok(temp.remote(script))).unwrap();
    assert!(temp.staged().is_empty());
}

#[test]
fn initial_factory_failure_returns_original_error_without_cleanup() {
    let temp = Temp::new();
    let source = temp.file("image.png", b"image");
    let mut calls = 0;
    let result = upload_files_with(
        &[source],
        &AtomicBool::new(false),
        |_, _| {},
        |_| {
            calls += 1;
            Err(Error::UploadIo(io::ErrorKind::PermissionDenied.into()))
        },
    );
    assert!(
        matches!(result, Err(Error::UploadIo(e)) if e.kind() == io::ErrorKind::PermissionDenied)
    );
    assert_eq!(calls, 1);
    assert!(temp.staged().is_empty());
}

#[test]
fn later_factory_failure_rolls_back_and_retains_cleanup_factory_failure() {
    for fail_cleanup in [false, true] {
        let temp = Temp::new();
        let sources = [temp.file("first", b"one"), temp.file("second", b"two")];
        let mut calls = 0;
        let error = upload_files_with(
            &sources,
            &AtomicBool::new(false),
            |_, _| {},
            |script| {
                calls += 1;
                match calls {
                    2 => Err(Error::UploadIo(io::ErrorKind::PermissionDenied.into())),
                    3 if fail_cleanup => {
                        Err(Error::UploadIo(io::ErrorKind::ConnectionRefused.into()))
                    }
                    _ => Ok(temp.remote(script)),
                }
            },
        )
        .unwrap_err();
        assert_eq!(calls, 3);
        if fail_cleanup {
            let Error::UploadCleanup { source, cleanup } = error else {
                panic!("expected combined error: {error:?}");
            };
            assert!(
                matches!(*source, Error::UploadIo(e) if e.kind() == io::ErrorKind::PermissionDenied)
            );
            assert!(
                matches!(*cleanup, Error::UploadIo(e) if e.kind() == io::ErrorKind::ConnectionRefused)
            );
            assert!(!temp.staged().is_empty());
        } else {
            assert!(
                matches!(error, Error::UploadIo(e) if e.kind() == io::ErrorKind::PermissionDenied)
            );
            assert!(temp.staged().is_empty());
        }
    }
}

#[test]
fn public_route_validation_and_empty_cleanup_do_not_call_factory() {
    let stop = AtomicBool::new(false);
    assert!(matches!(
        upload_files_with(
            &vec![PathBuf::new(); 257],
            &stop,
            |_, _| {},
            |_| panic!("factory")
        ),
        Err(Error::UploadPathLimit)
    ));
    assert!(matches!(
        remove_uploaded_files_with(&["/unowned/file".into()], |_| panic!("factory")),
        Err(Error::UploadCleanupPath)
    ));
    remove_uploaded_files_with(&[], |_| panic!("factory")).unwrap();
    assert!(matches!(
        remove_uploaded_files_with(&["/tmp/herdr-upload.abcdefghijkl/file".into()], |_| Err(
            Error::UploadCancelled
        ),),
        Err(Error::UploadCancelled)
    ));
}
