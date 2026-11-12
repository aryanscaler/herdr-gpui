use super::*;

#[test]
fn resuming_never_replaces_the_servers_effective_policy_with_auto() {
    for sandbox in [
        json!({"type":"readOnly","networkAccess":false}),
        json!({"type":"workspaceWrite","writableRoots":["/custom/root"],"networkAccess":true}),
        json!({"type":"dangerFullAccess"}),
    ] {
        let mut bridge = Bridge::default();
        let initialize = bridge.start();
        bridge.on_session(request(
            1,
            "session/load",
            json!({"sessionId":THREAD,"cwd":"/work","mcpServers":[]}),
        ));
        let ready = bridge.on_harness(&answer(&harness(&initialize[0])["id"], json!({})));
        let list = harness(&ready[1])["id"].clone();
        let resume = harness(&ready[2]);
        assert_eq!(resume["method"], "thread/resume");
        bridge.on_harness(&answer(&resume["id"], json!({"thread":{"id":THREAD},"model":"custom-model","reasoningEffort":"high","approvalPolicy":"never","sandbox":sandbox})));
        bridge.on_harness(&answer(&list, models()));
        let turn = prompt(&mut bridge, 2);
        assert!(turn["params"].get("approvalPolicy").is_none());
        assert!(turn["params"].get("sandboxPolicy").is_none());
        assert_eq!(turn["params"]["model"], "custom-model");
        assert_eq!(turn["params"]["effort"], "high");
    }
}

#[test]
fn only_an_explicit_mode_choice_overrides_the_session_policy() {
    let mut bridge = opened();
    let result = bridge.on_session(request(
        3,
        "session/set_mode",
        json!({"sessionId":THREAD,"modeId":"auto"}),
    ));
    assert!(response(&result[0], 3).is_ok());
    let turn = prompt(&mut bridge, 4);
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "workspaceWrite");
}
