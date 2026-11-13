# Teleport session continuity audit

This is an implementation acceptance record, not a claim that every agent
resumes across hosts. A handoff starts a new conversation; it is not session
continuity.

## Backend combinations

`sheprd-remote/src/teleport/job/plan.rs` selects the destination generation
independently of how the source session was discovered. `sessions.rs` carries
the native session storage; `job.rs` launches into either generation.

| Source → destination | Session identity today | Acceptance still required |
| --- | --- | --- |
| H1 → H1 | H1 integration reference, matched to the detected agent | Live transfer and return trip |
| H1 → H2 | Same H1 reference and transfer implementation | Live transfer and crash recovery |
| H2 → H1 | Shared owning-host reporters; native Claude fallback | Live transfer and return trip |
| H2 → H2 | Same source discovery as H2 → H1 | Live transfer and crash recovery |

Before a move and again immediately before retirement, resumable sessions of
both generations are re-read. Session reference (including provenance), retained
flags, and working directory must match the review. These are fresh checks,
not an atomic ownership guarantee: another client can change the process or
conversation between checking and stopping it.

Discovery consumes `sheprd-client::herdr::sessions::Binding` from
the owning host's `sheprd-acp session-discover TERMINAL AGENT`, using the same
socket/session environment as Teleport's core commands. The complete binding
(core, terminal, PID, birth token, selection sequence, provenance and config
metadata) is retained in the review and compared again before retirement.
Explicitly absent or cleared selections permit handoff; malformed, stale or
failed reads refuse the move. Missing helpers permit handoff, not guessed
session IDs. Hosts without the helper retain the legacy native Claude probe.
Claude's native record is attributed to Sheprd's own validation,
not impersonated as a Herdr 1 integration event.

This integration depends on [#1285](https://github.com/sheprd-labs/sheprd/pull/1285),
foundation through `08cbcfd6b`. It supplies all seven carried agents' reporters:
Claude/Codex/Copilot/Letta hooks, Pi/Omp extensions, and the OpenCode V2 TUI
plugin, plus native Claude discovery. Installing the helper alone
does not install those reporters. See [session evidence](agent-session-evidence.md).

## Agent coverage

The existing transferable subset is `AgentKind::carries` in
`sheprd-client/src/herdr/agent_kind.rs`:

| Agent | Transfer mechanism | Remaining evidence needed |
| --- | --- | --- |
| Claude | Project transcript and subagent archive | Live round trip; legacy hosts lack config evidence |
| Codex | Exact rollout filename suffix, preserving dated path | Installed SessionStart reporter, live round trip |
| OpenCode | Native export/import | Installed TUI reporter, live round trip and remote TUI topology |
| Pi / Omp | Exact session path or full filename ID suffix | Installed extension, PI_CONFIG_DIR semantics, live round trip |
| Copilot | Whole session-state directory | Installed SessionStart reporter, live round trip |
| Letta | Server-side conversation reference | Installed hook and destination server/account identity |

Duplicate Claude, Codex, Pi, or Omp files for one reference are refused rather
than choosing the first file. Pi/Omp ID lookup is limited to the session tree
and matches the whole ID suffix, not a substring. Transfer rejects references
whose agent, reference kind, or provenance is not accepted for that agent.

Other `AgentKind` variants already lack confirmed cross-host storage support
for H1. Preserving H1's *same-host* resume commands does not establish that
their history is available on a different host. Each requires a documented
storage/export contract before being added to the transferable subset.

## Host and return paths

- Local, saved SSH and cloud hosts execute discovery and transfer on the
  selected host. A disconnected app-kept destination probes Git directly;
  it does not query a nonexistent H1 inventory.
- Teleport back uses `Destination::Reclaim`, checks repository identity, backs
  up the checkout, and chooses the keeper of the original workspace. Session
  transfer is the same in both directions; it is not a special resume shortcut.
- WSL and custom socket hosts are explicitly rejected by `Host::new`.
  Native Windows Teleport is not implemented. These are not covered by macOS
  fixture tests.
- Evidence's configuration roots under the source home or checkout map to the
  corresponding destination home or checkout. Source reads, destination writes,
  retained path-valued flags, and resume commands use those explicit roots.
  Config files and credentials are not copied: an `OPENCODE_CONFIG` file must
  already exist at its mapped destination. Roots outside those locations and
  `PI_CONFIG_DIR` are refused pending a confirmed mapping. Legacy H1 and
  helper-free native Claude transfer scripts still use the host shell
  environment; foreground-only configuration overrides remain to be handled.

## Commit, rollback and durable arrival

`job::run` now copies idle sessions while their source programs remain alive.
Copy or launch failure does not start a fresh substitute. The destination must
report the exact transferred reference through its own trusted evidence; a
queued line or merely detecting the same agent is insufficient. A bounded wait
refuses unacknowledged resumes.

Before retirement, app-kept destination layouts are written to private,
size-bounded, synced receipts under `teleport-arrivals` beside the app's saved
tabs. Once the receipt is committed, cancellation cannot erase the destination.
Retirement failures leave it running, warn, and do not authorize the UI to close
remaining source tabs or mark the source retired.

The app recovers receipts on a worker and adopts existing destination terminals
idempotently, including partial prior adoption. It never relaunches agents or
retires sources during recovery. Receipts are removed only after the correct
host's tabs are saved and synced. A full workspace/tab list retains the receipt
and running destination for retry, rather than killing it.

Before that commit point, rollback ends only newly owned destination terminals
and H1 tabs. Transferred checkout data and imported session files can remain
for inspection; this is source-preserving failure handling, not rollback of
all destination filesystem writes. A crash after persisting but before source
retirement can leave both copies running; recovery deliberately does not guess
which source process is still owned.

## Remaining limits

- Conditional PID/birth/session-bound core stop is an upstream requirement.
  Fresh checks narrow but cannot eliminate replacement between checking and
  retirement. No external Herdr checkout is changed by this work.
- Session/config stores already in concurrent use on the destination need
  additional collision handling; the receipt is not a filesystem transaction.
- Receipts restore destination tabs, but not a Teleport-back mark that the
  app never saved before a crash. Persisting the source/return relationship
  with an acknowledged retirement phase remains follow-up work.
- Config roots outside home/checkout, PI_CONFIG_DIR semantics, legacy H1 config
  metadata, Letta custom server/account scope, WSL/Windows Teleport, and remote
  OpenCode TUI topology remain explicitly unverified or unsupported.
- Installing and exercising every reporter on both hosts remains required.
  Startup argv, newest transcript selection and fabricated provenance are never
  substitutes for missing evidence.

## Validation

Deterministic tests cover mismatched agent/provenance, changed flags/cwd,
duplicate session files, complete Pi ID matching, cancelled server-backed
references, and daemon-free destination discovery. Existing script fixtures
cover Claude, Codex, Copilot and Pi transfer formats. These fixtures do not
prove a real agent resumed the same conversation.

Pending: live H1/H2 four-way transfers and return trips; multiple concurrent
agents with different roots; source replacement during transfer; cancellation
at every phase; destination restart or app crash before arrival persistence;
SSH/cloud execution; platform CI; native UI checks. Overall parity remains
incomplete until the dependencies and lifecycle gaps above are closed.
