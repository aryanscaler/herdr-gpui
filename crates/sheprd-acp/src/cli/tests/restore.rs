use super::*;

#[test]
fn restore_names_one_exact_endpoint_without_launch_options() {
    let id = "0123456789abcdef0123456789abcdef";
    assert_eq!(
        parse(args(&["restore", id])).unwrap(),
        Parsed::Restore(id.parse().unwrap())
    );
    assert_eq!(parse(args(&["restore"])), Err(Error::MissingEndpoint));
    assert_eq!(
        parse(args(&["restore", "latest"])),
        Err(Error::InvalidEndpoint("latest".into()))
    );
    assert_eq!(
        parse(args(&["restore", id, "--cwd", "/other"])),
        Err(Error::UnknownOption("--cwd".into()))
    );
}
