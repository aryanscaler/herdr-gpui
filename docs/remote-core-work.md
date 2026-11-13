# Work commands from remote Herdr 2 terminals

The managed `sheprd`/`herdr-gpui` shim sends app-kept work commands from an
SSH, cloud, or WSL core terminal over the authenticated core connection the
desktop already uses. It requires Python 3 and a driving tab in an open
Sheprd window. It opens no socket or browser server on the remote host.

Supported entrypoints are `worktree status|mute|unmute|list|create|open|delete`,
`notes list|get|set|clear`, `scripts setup|run`, `checkpoint restore`,
`teleport --to HOST`, and `fan-out`. `--json` and `--window ID` are honored.
Archive scripts remain behind the worktree deletion dialog. The shim does
not implement the full CLI's local Git commands: checkpoint list/create/diff
and scripts list require the full binary on the host.

## Identity and confirmation

- Requests contain only protocol version, optional window ID, and a typed
  `WorkOp`. They cannot supply a caller, daemon socket, terminal, workspace,
  source host, or arbitrary control operation.
- The app derives host and workspace from the remote core's terminal and the
  tab that drives it. Mirrors and terminals outside kept workspaces refuse.
  Identical terminal IDs on different hosts are different callers.
- An explicitly named window must drive that caller on the same host. Dialogs
  require the caller's host to be selected; they never switch to another host
  to satisfy a request. Notes listings contain only the caller's host's notes.
- Restore, Teleport, fan-out, create, open, and delete prepare the normal app
  dialogs. The user still confirms. Scripts retain repository-content trust;
  a previously trusted script runs only on the caller's captured host.

## Transport and bounds

`sheprd-work-NONCE` contains JSON, where NONCE is a fresh 32-hex-digit value.
`sheprd-work-reply-NONCE` contains the ordinary typed control response. This
namespace is independent of browser requests. Requests are limited to 4 KiB,
replies to 16 KiB, and a window retains at most eight pending requests.
The shim bounds each core read to 1 MiB and ten seconds while streaming.
It waits at most 30 seconds overall, prints one answer, and removes its own
request/reply labels on completion or cancellation. It never retries work.

A new or replaced connection ignores the first list requested after that
connection was observed. Old labels, labels on undriven terminals, and labels
present during a disconnect are not replayed later. A bounded per-connection
ledger remembers observed request names; overflow suspends that connection's
work discovery rather than forgetting a request and replaying it.

Before dispatch, a worker rereads the request on the captured core and marks
its label `claimed`. That marker is not an executable request. Pending work
checks the exact backend connection, saved host target, caller tab, and
deadline before dispatch. Windows in the same app share claims. A lost reply
does not replay the operation: inspect the app before submitting another
command. Cancellation after dispatch cannot undo work already prepared or a
trusted script already launched.

## Validation

- Protocol tests reject caller/host overrides, unknown operations and
  versions, malformed nonces, response-name confusion, and oversized JSON.
- `python3 scripts/tests/test_remote_work.py` runs the assembled installed
  shell entrypoint against a fake core: success, confirmations, window/busy
  refusals, invalid/oversized replies, cancellation, signal cleanup, exactly
  one request, and no host override.
- Headless adapter tests cover remote driving-tab identity and host-filtered
  notes; ledger tests cover reconnect suppression and saturation. Their
  compilation is checked separately from execution.
- Native SSH/cloud/WSL runtime checks and current-head CI remain required;
  fake-core tests are not native end-to-end verification.

The implementation is stacked on the work adapters (#1287), host domain
operations (#1286), fan-out/dispatch integration (#1281), and the browser
label transport foundation (#1291). It reuses the latter's label read/write
workers without treating browser metadata as work authority.
