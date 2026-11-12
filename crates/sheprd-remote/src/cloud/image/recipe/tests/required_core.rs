use super::*;
use crate::cloud::{Requirement, image::key};

#[test]
fn required_core_images_preserve_project_user_and_separate_cached_images() {
    let source = "FROM debian\nUSER developer\n";
    let core = compose_for(source, Requirement::Core).unwrap();
    let either = compose_for(source, Requirement::Either).unwrap();
    assert!(core.starts_with(source));
    assert!(core.ends_with("USER developer\n"));
    assert!(core.contains("herdr-core"));
    assert!(!core.contains("herdr.dev/install.sh"));
    assert!(!core.contains("|| echo"));
    assert_ne!(
        key::key(&core, &[], "linux/amd64"),
        key::key(&either, &[], "linux/amd64")
    );
}
