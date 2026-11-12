use super::*;

#[test]
fn codex_cli_choices_become_app_server_config_without_replaying_prompt() {
    let argv = [
        "codex",
        "--model",
        "gpt-5",
        "--sandbox=read-only",
        "--ask-for-approval",
        "never",
        "fix it",
    ]
    .map(str::to_owned);
    assert_eq!(
        super::super::flags::chat(AgentKind::Codex, &argv).unwrap(),
        [
            "-c",
            "model=\"gpt-5\"",
            "-c",
            "sandbox_mode=\"read-only\"",
            "-c",
            "approval_policy=\"never\""
        ]
    );
}

#[test]
fn copilot_keeps_its_explicit_config_directory() {
    let argv = [
        "copilot",
        "--config-dir",
        "/custom",
        "--model",
        "model-a",
        "--resume=old",
    ]
    .map(str::to_owned);
    assert_eq!(
        super::super::flags::chat(AgentKind::Copilot, &argv).unwrap(),
        ["--model", "model-a", "--config-dir", "/custom"]
    );
}

#[test]
fn unknown_server_options_refuse_instead_of_changing_policy() {
    let argv = ["codex", "--future-permission-mode"].map(str::to_owned);
    assert!(super::super::flags::chat(AgentKind::Codex, &argv).is_err());
    let argv = ["opencode", "--model", "model-a"].map(str::to_owned);
    assert!(super::super::flags::chat(AgentKind::Opencode, &argv).is_err());
}
