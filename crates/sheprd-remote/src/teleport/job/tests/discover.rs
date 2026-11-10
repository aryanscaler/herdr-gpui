use super::*;
use crate::teleport::remote::parse_remotes;

fn identity(name: &str, origin: Option<&str>) -> RepoIdentity {
    let remotes = origin.map_or_else(Vec::new, |url| {
        parse_remotes(&format!("origin\t{url} (fetch)\n"))
    });
    RepoIdentity {
        name: name.into(),
        remotes,
    }
}

#[test]
fn a_repository_is_named_after_its_folder() {
    assert_eq!(repository_name("/home/me/app/.git"), "app");
    assert_eq!(repository_name("/home/me/app/.git/"), "app");
    assert_eq!(repository_name("/srv/git/app.git"), "app");
}

/// A checkout work left is reclaimed only for the same project: the same
/// branch name in another repository is never reset to this work.
#[test]
fn only_the_same_project_is_reclaimed() {
    let ours = identity("app", Some("git@github.com:me/app.git"));
    let same = identity("app", Some("https://github.com/me/app"));
    assert_eq!(same_project(&ours, &same), Some(MatchReason::Origin));
    let other = identity("app", Some("git@github.com:someone/other.git"));
    assert_eq!(same_project(&ours, &other), None, "same name, other remote");
    assert_eq!(same_project(&ours, &identity("app", None)), None);
    assert_eq!(same_project(&ours, &identity("lib", None)), None);
    // Without remotes to compare, the name decides.
    let local = identity("app", None);
    assert_eq!(
        same_project(&local, &identity("app", None)),
        Some(MatchReason::Name)
    );
    assert_eq!(same_project(&local, &identity("lib", None)), None);
}
