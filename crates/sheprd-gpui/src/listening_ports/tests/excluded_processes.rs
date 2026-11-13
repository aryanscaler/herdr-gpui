use super::*;

#[test]
fn agent_listeners_do_not_create_a_workspace_port_row() {
    let ports = parse(
        "L 10 127.0.0.1:63644 codex\n\
         L 10 [::1]:63644 codex\n\
         L 11 127.0.0.1:61031 2.1.322\n\
         L 12 127.0.0.1:62257 node\n\
         L 13 127.0.0.1:62258 opencode\n\
         E 10 w1\nA 10 /opt/homebrew/bin/codex\n\
         E 11 w1\nA 11 claude --resume\n\
         E 12 w1\nA 12 node /usr/lib/node_modules/@anthropic-ai/claude-code/cli.js\n\
         E 13 w1\nA 13 opencode\n",
    )
    .unwrap();
    assert!(ports.ports.is_empty());
}

#[test]
fn the_workspace_that_started_an_agent_is_still_known() {
    let ports = parse("L 10 127.0.0.1:61031 2.1.322\nE 10 w1\nA 10 claude\n").unwrap();
    assert_eq!(ports.pids[&10].workspace, "w1");
}

#[test]
fn agent_listeners_in_a_core_terminal_are_dropped() {
    let ports = parse("L 10 127.0.0.1:61031 2.1.322\nC 10 t1\nA 10 claude\n").unwrap();
    assert!(ports.terminals.is_empty());
    assert_eq!(ports.terminal_pids[&10].terminal, "t1");
}

#[test]
fn an_agent_is_known_by_its_name_without_a_command_line() {
    let ports =
        parse("L 10 127.0.0.1:63644 codex\nL 11 127.0.0.1:62257 claude\nE 10 w1\nE 11 w1\n")
            .unwrap();
    assert!(ports.ports.is_empty());
}

#[test]
fn servers_an_agent_starts_remain_visible() {
    let ports = parse(
        "L 11 *:5173 node\n\
         L 12 127.0.0.1:8000 python3.12\n\
         L 13 127.0.0.1:9000 1.4.2\n\
         E 11 w1\nA 11 node /work/app/node_modules/.bin/vite\n\
         E 12 w1\nA 12 python3.12 -m http.server\n\
         E 13 w1\nA 13 /opt/tool/versions/1.4.2 serve\n",
    )
    .unwrap();
    assert_eq!(
        of(&ports, "w1"),
        [
            port(5173, Bind::Any, "node"),
            port(8000, Bind::Loopback, "python3.12"),
            port(9000, Bind::Loopback, "1.4.2"),
        ]
    );
}

#[test]
fn an_agent_does_not_hide_another_process_on_the_same_port() {
    let ports = parse(
        "L 10 127.0.0.1:5173 codex\n\
         L 11 [::1]:5173 node\n\
         E 10 w1\nA 10 codex\nE 11 w1\nA 11 node server.js\n",
    )
    .unwrap();
    assert_eq!(of(&ports, "w1"), [port(5173, Bind::Loopback, "node")]);
}
