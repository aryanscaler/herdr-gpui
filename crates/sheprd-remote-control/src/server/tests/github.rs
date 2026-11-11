use super::*;

#[test]
fn github_cli_reads_require_explicit_host_local_opt_in() {
    assert!(!serve(&["serve", "--state-dir", "/s"]).github_cli);
    assert!(serve(&["serve", "--state-dir", "/s", "--github-cli"]).github_cli);
    assert!(USAGE.contains("--github-cli"));
    let config: crate::app::config::RemoteConfig = toml::from_str("github_cli = true").unwrap();
    assert!(config.github_cli);
    assert!(!crate::app::config::RemoteConfig::default().github_cli);
}
