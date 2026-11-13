# Remote core browser bridge

Remote Herdr 2 browser requests reuse the authenticated core connection already
owned by the desktop. No remote browser server or desktop control socket is
installed. The managed POSIX shim works in SSH, cloud, and WSL core terminals.
Its terminal must be in a driving desktop tab; observer tabs cannot originate
requests. The desktop must stay open and connected.

## Contract and ownership

`core_terminals/requests.rs` discovers fresh request labels and retains the
existing first-snapshot and unshown-terminal fences. The desktop derives the
scope, workspace, and terminal from the connected backend and driving tab. It
ignores a pane claimed by the request. `chat::core_pane::key_on` qualifies the
origin with the host, so `t3` on two machines is never one recipient.
Shared review/note delivery and the desktop control feedback lookup use the
same exact driving-pane lookup for remote core keys, independently of the
selected Herdr 1 session. An unheld remote key cannot fall back to that session
or alias a local `core:t3`. The remote shim continues to use its authenticated
terminal-label route, rather than creating a local caller.

The existing `herdr-gpui-ID` header accepts `open`, `open-background`, and
`reload`. URLs remain split into up to twelve `_N` labels. Reload only targets
pages opened by the same host-qualified origin in that scope. Opening a page
still reports that it was *requested*, not acknowledged as displayed.

Feedback adds `feedback-N` (0–600 seconds) to that header. The remote shim
generates a random 12-hex-digit ID. `remote_browser/feedback.rs` owns at most
eight pending callers and one per recipient. The response label is
`sheprd-reply-ID`, containing JSON `{"notes": STRING_OR_NULL}` within the
existing 240 KiB notes budget. The caller polls only its own terminal with
`herdr-core get --json`, with bounded subprocess deadlines and a 1 MiB read
limit, prints and flushes the reply, then replaces its request value with `ack`.
The desktop removes both labels after acknowledgement. The caller exits 4 for
an empty response, 1 on transport failure, and 130 on interruption.

Cancellation removes the caller's request and response. A disconnected core,
closed owner window, failed reply write, or missing acknowledgement restores
offered notes ahead of newer notes. A reply waits 30 seconds for acknowledgement.
An acknowledgement lost after printing may cause duplicate delivery on retry;
the protocol prefers a duplicate to silently dropping notes. Notes remain
in-memory like the local feedback queue, not durable across app termination.
Requests refused by the pending-call bound expire in the caller without taking
notes. Neither first-connection labels nor requests from unshown terminals are
replayed later.

Other remote CLI domains must use a separate typed namespace and dispatcher.
Browser labels grant no worktree authority. A work request must independently
validate its operations and preserve the app's confirmation requirements.

## Scope and remaining validation

- Supported: remote open, background open, reload, and annotation feedback,
  including bounded wait and returned notes, without Herdr 1.
- Annotation screenshots use the file-images host transfer adapter (#1288).
  The batch is limited to 16 MiB before saving/uploading; the original caller,
  backend connection, host configuration and page are revalidated before
  delivering quoted remote paths. Unaccepted copies are removed using the
  original host and unmodified returned paths. WSL maps private local artifacts,
  which remain in the existing age-cleaned annotation directory. Failed
  transfers return textual notes with a native warning, never local file paths.
  SSH/cloud image uploads currently require a Linux/macOS desktop client;
  Windows supports the WSL path and reports unsupported SSH/cloud transfers.
- SSH loopback pages use the existing SSH forward; WSL uses localhost
  integration. Cloud endpoints without an SSH forward require reachable URLs.
- Remote file URLs, beside placement, arbitrary local control commands, and
  selecting another caller/window are refused by the shim.
- `scripts/tests/test_remote_browser_feedback.py` exercises the actual shim
  against a deterministic core CLI peer: multiline Unicode notes, exact ack,
  empty replies, cancellation/disconnect cleanup, and invalid wait bounds.
- Rust state-machine tests cover host isolation, wait expiry, acknowledgement,
  failed writers, cancellation, and restoring notes before newer batches.
  Request tests cover restart/unshown fences and exact driving-pane lookup.
- Native SSH/cloud/WSL UI checks and default/all-feature/platform builds remain
  CI/manual checks until their results are recorded in the PR. No native desktop
  interaction was performed for this foundation change.
