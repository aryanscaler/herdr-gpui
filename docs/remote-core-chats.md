# Remote Herdr 2 native chats

Owner: `feat/h2r3-remote-chats`. This is implementation and acceptance evidence,
not a claim that all runtime/platform checks have passed.

## Routing and ownership

- `chat/core_pane.rs` discovers ACP endpoint labels in driven core tabs on
  their owning host. Local keys stay `core:t1`; remote keys include a
  length-delimited saved host scope. `Surface::RemoteCore` retains the typed
  host and terminal separately from the display key.
- `core_terminals/hosts.rs` reads each host's own terminal metadata. A remote
  `t1` never borrows the local `t1`'s cwd, foreground process, or tab. Mirrors
  neither split nor display an interactive chat. Normal split/zoom layout and
  native composer focus apply to remote panes.
- A remote launch executes the existing `remote_program_script` lookup on
  that host, with the endpoint environment and terminal label chosen together.
  It first reuses this release's ACP installed by the existing phone-pairing
  installer, then searches the host's user bins, app bundles and PATH.
  The existing core start queue supplies the remote cwd and tab/workspace
  association. A kept workspace without a driven tab starts a new core tab.
- SSH attach uses the existing `sheprd-acp connect ENDPOINT` stream. Cloud
  and WSL use `chat/remote_route.rs`: provider credentials remain alive for
  the entire stream; WSL uses the system WSL executable and the selected
  distribution's shell. Endpoint IDs are validated, never treated as paths.
- Changing/removing the saved host invalidates discovery against metadata
  from its former connection. Process-less metadata, recovered shells, lost
  terminals, and core-forwarded `other/t1` terminals are not chat endpoints.
- Prompts, permission answers, cancellation, bounded queues, cursor resume,
  and command deduplication use the existing chat session/link protocol.
  Dropping a link cancels connector preparation and closes its exact child.
  This transport never launches recovery when a stream drops.
- A cancelled start is acknowledged only when its window takes ownership of
  the result. Unclaimed results (including buffered ones) are cleaned up by
  the spawn worker on its captured core, never by retrying a terminal ID
  against an edited host profile. A failed cleanup after transport loss is
  logged; the app cannot claim the unreachable host ended that process.

## Conversion and recovery dependencies

Remote conversion runs `sheprd-acp session-convert TERMINAL AGENT` on the
owning host, with `SHEPRD_EXPECT_CORE` set to the connected core's ID. It keeps
the host's login-selected `HC_SOCKET`/`HC_SESSION`, as the core bridge does,
and clears inherited terminal attribution. The helper validates that core
before discovery or mutation. Its session discovery belongs to #1285; see
[agent session evidence](agent-session-evidence.md) for supported reporters
and limitations. Conversion is cancelled if the saved target or core changes,
and is never automatically replayed after a transport failure.

Recovery records, ownership claims, explicit restore, and expected-close
handling belong to the chat-restore slice (#1280). Remote launch uses the
host core's injected `HC_TERMINAL_ID` and `HC_TERMINAL_SOCKET`; it neither
copies records to the desktop nor starts a second agent to reconnect a view.
Automatic process-loss/reboot recovery is not proven by these attach changes.

## Acceptance evidence

| Area | Regression coverage | Runtime status |
| --- | --- | --- |
| Host-scoped identities | `chat/core_pane/tests/identity.rs`, `tests/remote.rs` | Rust execution pending CI |
| Remote split/visibility and mirror refusal | `core_terminals/tests/chats/remote.rs` | Rust execution pending CI |
| Native focus, host removal, prompt/answer/cancel routing | `chat/tests/core_panes/remote.rs` (headless, mock peer) | Pending CI; not native desktop proof |
| Endpoint validation and recovered-shell exclusion | `chat/core_pane/tests/remote.rs` | Pending CI |
| Connector cancellation, invalid targets, no local fallback | `chat/remote_route/tests.rs` | Pending CI; WSL runtime requires Windows |
| WSL script boundary | `sheprd-client/src/wsl/tests/script.rs` | Default/all-feature macOS runs: 2 passed each; Windows construction pending CI |
| Cancelled launch ownership | `core_terminals/start/tests.rs` | Pending CI; accepted starts stay running, unclaimed successes end only their created terminal |
| Owning-core conversion fence | `chat/convert/remote/tests.rs` and #1285 helper tests | Pending CI and remote agent smoke tests |

Manual acceptance still required: start each supported agent on saved SSH,
cloud and WSL hosts with no Herdr 1; submit prompts and permission answers;
hide/show and resize narrow split panes; disconnect/reconnect without duplicate
turns; switch/edit/remove hosts with identical terminal IDs; close/reopen the
desktop; verify explicit recovery and intentional closure against #1280.
Native screenshots must come from the built application on a coordinated
desktop, not a headless rendering fixture.

Prerequisites and limitations: the host needs its own ACP and agent binaries.
The route requires POSIX `/bin/sh`; native Windows SSH shells without it are
unsupported. WSL execution is Windows-only. Non-Claude conversion requires
the exact process-bound evidence supported by #1285, not guessed session IDs.
Phone conversion and standalone layout are separate owned slices. Remote
attachments consume the file-images transfer foundation; native Windows
SSH/cloud uploads retain its existing explicit unsupported result.
