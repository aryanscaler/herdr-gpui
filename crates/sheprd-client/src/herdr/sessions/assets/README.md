# Sheprd-owned session reporting assets

These assets are inert until explicitly wired by the managed installer. They do
not modify agent configuration themselves and contain no Claude integration.

- Invoke each POSIX hook with `sh /absolute/managed/path/<agent>-session-start.sh`
  only for that agent's **SessionStart** event. Stdin is the original event JSON.
  The hooks `exec` `sheprd-acp session-report codex|copilot`, retaining the agent
  as the parent for CLI identity validation. The CLI must bound stdin, require
  Codex `session_id` plus `transcript_path`, exclude a conflicting
  `CODEX_THREAD_ID`, and accept Copilot `session_id` or `sessionId`. Event filtering
  and validation must also be enforced by the installer/reporter contract; these
  shell adapters deliberately do not attempt to parse JSON.
- Install the entire `opencode/` directory as a V2 **TUI-only** plugin package.
  Its `./tui` export uses the host-resolved `@opencode/plugin/tui` SDK. Wire the
  package through V2 CLI plugins, not a server plugin. Only the selected TUI route
  determines the session; background server events and session lists are unused.
- Both integrations require `HC_TERMINAL_ID`. No legacy Herdr environment
  variable substitutes for it. Scope and process identity verification belong
  to the CLI. The plugin emits `{pid, session_id, sequence}` with `null` denoting
  an explicit clear; the sequence survives plugin reload through TUI memory.
- The managed launcher may set `SHEPRD_ACP_BINARY` to its absolute companion
  binary. Otherwise `sheprd-acp` is resolved on PATH. Do not populate this variable
  from hook input. The plugin and shell adapters ignore non-absolute overrides.

The documented V2 router has no change subscription, so the plugin samples every
250 ms, deduplicates, and retains only the latest pending report while one child
runs. Each reporter child has a 1.5-second termination deadline. Delivery is
best-effort; missing binaries, process shutdown, and failed reporting do not
block the TUI. Cleanup reports a clear, but a hard process kill cannot run cleanup.
The CLI must reject stale sequences and stale process identities. Hook read
deadlines and payload limits are enforced by the CLI, not by these adapters.

Run deterministic tests without OpenCode or a daemon:

```sh
node --test crates/sheprd-client/src/herdr/sessions/assets/tests/*.test.mjs
```

The plugin API follows https://opencode.ai/v2/docs/build/plugins/cli and
https://opencode.ai/v2/docs/build/plugins . Mock tests do not verify loading in a
real OpenCode V2 TUI or real Codex/Copilot hook invocation.
