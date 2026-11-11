use super::*;

#[test]
fn service_only_passes_github_permission_when_explicitly_enabled() {
    let mut plan = plan();
    assert!(!plan.args().iter().any(|arg| arg == "--github-cli"));
    plan.config.github_cli = true;
    assert!(plan.args().iter().any(|arg| arg == "--github-cli"));
    plan.config.github_cli = false;
    assert!(!plan.args().iter().any(|arg| arg == "--github-cli"));
}
