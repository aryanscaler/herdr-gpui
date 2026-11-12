# Herdr 2 agent session evidence

Herdr 2 does not expose the active agent session in its terminal snapshot.
Sheprd's opt-in session integrations record explicit selections in the owning
core's document store. The desktop, phone daemon and Teleport helper can read
the same record while the desktop is closed. They never choose the newest
session, search by working directory, or use startup argv as current identity.

## Explicit installation

On the machine running the agent, using that installation's `sheprd-acp`:

```sh
sheprd-acp session-install codex
sheprd-acp session-install copilot
sheprd-acp session-install opencode
sheprd-acp session-install pi
sheprd-acp session-install omp
sheprd-acp session-install letta
```

Run only the commands for integrations you want installed, then restart those
agents. Installation preserves other hooks and settings, uses separately named
Sheprd assets, and is idempotent. Codex uses `CODEX_HOME`, Copilot uses
`COPILOT_HOME`, and OpenCode's TUI config uses `XDG_CONFIG_HOME`. Invalid JSON,
JSONC comments and unexpected config shapes are refused before writing assets;
they are not rewritten as empty configurations. Linux/macOS use POSIX hooks;
Windows reporters use PowerShell and CIM birth tokens. Windows native conversion
still lacks the matching shell launch policy. No integration is installed automatically.

The installer pins its own helper executable path. Reinstall after moving or
removing that binary. Installation on SSH/cloud/WSL machines must use the helper
on that machine, with the intended agent configuration environment.

## Shared contract

`sheprd_client::herdr::sessions::Binding` version 1 carries:

- the owning core ID and terminal address;
- foreground PID and host-local process birth token;
- agent kind, exact session reference, and explicit `sheprd:session-v1` provenance;
- OpenCode's monotonic selection sequence;
- allowlisted configuration-directory variables, never credentials.

`Binding::parse` validates bounded data against the expected identity.
`AgentKind::accepts` recognizes this provenance without pretending it came from
Herdr 1. Reference kinds include IDs and Pi/OMP session paths. Producers currently
exist for Codex/Copilot SessionStart events and OpenCode V2 TUI selection. The
Pi/OMP extensions report exact session-manager paths with selection sequences;
Letta's direct hook reports explicit conversations and qualifies default ones
with their agent ID. Claude also has an opt-in reporter for custom config roots.

The reporter verifies that it descends from the current foreground agent, with
only shell hook launchers intervening. Inherited `HC_TERMINAL_ID` alone is not
proof. OpenCode reports the terminal client's route, including explicit clears;
shared-server session-created events cannot establish which terminal selected it.
Writes use document compare-and-swap. Conflicting unordered SessionStart events
clear evidence for that process rather than guessing which event is newest.

Host-local helper commands:

```sh
sheprd-acp session-report AGENT       # hook JSON on stdin; injected HC_TERMINAL_ID/SOCKET required
sheprd-acp session-discover TERMINAL AGENT
sheprd-acp session-convert TERMINAL AGENT
```

Discovery prints binding JSON and exits 0; absent or explicitly cleared evidence
exits 3; invalid/stale evidence or transport failures exit 1; CLI errors exit 2.
Use the same `HC_SOCKET`/`HC_SESSION` as the owning core. Routed addresses are
rejected: execute the helper on the owning host instead of reading a local PID.
Reporting is different: it requires and uses the injected `HC_TERMINAL_SOCKET`,
ignoring shell endpoint/session overrides so it reports to its actual owner.
Remote callers must clear unrelated inherited `HC_SOCKET`/`HC_SESSION` before
selecting the intended endpoint, and set `SHEPRD_EXPECT_CORE` to the core ID from
their captured connection. A different core fails before discovery or mutation.
Conversion prints the chat endpoint. It uses the same worker policy as desktop
and phone local conversion and leaves unknown/unsupported sessions running.

Configuration paths belong to the source host. Conversion on that host retains
them. Teleport must map source roots to reviewed destination roots; copying an
absolute source path into the destination environment is not valid relocation.

## Verified boundaries and remaining work

Mock-core regression tests cover Codex, Copilot and OpenCode conversion using an
exact bound session, including a custom Codex home. Pure tests cover scope and
birth mismatch, bounds, explicit clears, provenance, config-variable filtering,
installer preservation and idempotence, and CLI parsing. Native agent hook runs,
phone end-to-end, remote conversion and live OpenCode plugin loading still need
verification. Native Windows conversion, remote OpenCode service attribution,
and full agent flag mapping remain incomplete. Unsupported
server flags refuse conversion rather than silently changing launch policy.
Codex's initial chat mode keeps the server's effective session policy: it sends
no sandbox/approval overrides until the user explicitly chooses a mode. The
effective model and reasoning effort are retained even for custom models absent
from the model catalog.

Linux birth tokens use `/proc/PID/stat` start ticks. macOS uses the kernel's
microsecond birth stamp through the maintained safe `libproc` wrapper; Windows
uses CIM creation ticks. These are opaque identity tokens, not mutation locks.
Claude's legacy PID file requires a matching `procStart`; custom config roots
need explicit reporting rather than a guessed file location.

### Upstream mutation fence blocker

The inspected `herdr-core schema` exposes `terminal.keys` parameters
`caller`, `keys`, `terminal`, with no expected process or status. `terminal.send`
has an `if` status condition (`idle`/`working`) but no PID, process-start token,
session revision or shell-owner condition. Both may wait for terminal quiet
before delivering input. `Foreground` has PID/name/argv/cwd, not birth identity.

Sheprd re-reads identity, selection and idle status before interruption and
checks the original shell before sending. These are **not atomic**: another
program can take the terminal after the last read, including during the core's
quiet wait. Complete safety needs an upstream compare-and-act precondition
checked at actual key/text delivery, tied to terminal generation, process birth,
and expected state/selection. Teleport retirement needs an equivalent conditional
kill. This blocker is separate from remaining Sheprd implementation work.
