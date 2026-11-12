# Herdr 2 clipboard images and file delivery

Herdr 2 terminal views acquire clipboard contents on a background worker using
the same bounded native reader and image preparation as Herdr 1. Copied files
take precedence over text, then bitmap images. PNG/JPEG/GIF/WebP/BMP and TIFF
clipboard conversion follow the existing image limits and resize policy.
Screenshot-thumbnail drops are staged before their temporary source disappears.
Ordinary local files still paste quoted paths, without reading or copying them.
No drop or image paste sends Enter.

## Destinations

| Destination | Delivery |
| --- | --- |
| Local core | A private temporary file for bitmap/thumbnail bytes; quoted original paths for ordinary files |
| Saved SSH core | The existing bounded file uploader, using the saved SSH trust policy; only completed remote paths are pasted |
| Saved cloud core | The same uploader through the provider's authenticated command, with credentials retained through rollback |
| WSL core | The selected distribution's `wslpath` conversion; ordinary Windows files are not copied or deleted |

SSH/cloud uploads require a POSIX destination and a Linux or macOS client.
**Native Windows SSH/cloud upload remains `UploadUnsupported`, an existing
Herdr 1 platform limit.** This change does not claim that coverage. Windows
local image staging and WSL path mapping are supported. Provider connection and
trust failures remain errors; a remote destination never falls back to pasting
this computer's file path.

Preparation belongs to the original view and connection. Reconnect, host-route
changes, and view destruction cancel it. Input typed during preparation is
bounded and waits behind it; a failed preparation discards that input. A result
from an old connection cannot enter a replacement view. Unclaimed newly staged
files are cleaned up on workers; WSL originals are never cleanup candidates.
Accepted temporary files persist for the receiving program and subsequent OS or
user cleanup, as successful Herdr 1 file uploads do.

Remote chat attachments use the same transfer adapter and compare the entire
captured chat target before accepting results. Cloud/WSL chat route selection
depends on the native remote-chat parity change. The clipboard operation is a
native Paste/drop interaction; it adds no CLI command or daemon dependency.

## Acceptance evidence

Deterministic coverage is in `sheprd-client/src/upload/tests/routes.rs`,
`core_terminals/files/tests.rs`, and `core_terminals/tests/file_images.rs`.
It covers binary transfer, shell-sensitive filenames, validation before I/O,
rollback errors, private bitmap staging, ephemeral screenshot sources, input
ordering, rejection, and stale host/connection results. Existing uploader tests
cover cancellation, source changes, malformed replies, and exact cleanup paths.

The following native checks remain required; unchecked items are **not verified**:

- [ ] With no Herdr 1 installed/running, paste PNG and TIFF clipboard images
  into local Herdr 2 panes; verify the receiving program reads their bytes.
- [ ] Drag a live macOS screenshot thumbnail; save/dismiss the thumbnail and
  verify the staged image remains readable. Repeat with a normal image file.
- [ ] Paste/drop files with spaces, quotes, Unicode and shell metacharacters
  into SSH and cloud panes; inspect bytes and paths on the named remote host.
- [ ] On Windows, paste bitmap images locally and into WSL; drop drive and
  distribution paths, including spaces; verify the selected distro reads them.
- [ ] Verify Windows SSH/cloud transfers report unsupported without pasting
  local paths or attaching them to remote chats.
- [ ] During a transfer, type text/Enter, switch hosts, reconnect/restart the
  core, and close the pane/window. Verify ordering and unclaimed-file cleanup,
  including two cores with the same terminal ID.
- [ ] Repeat Paste through the Edit menu and keyboard shortcuts, with focus in
  a narrow split, a popup/composer, and during IME composition. Mirrors must
  remain read-only and another pane must receive no input.
- [ ] Repeat with Herdr 1 and Herdr 2 coexisting; verify destination isolation.
- [ ] Validate remote chat attachments with the remote-chat dependency, including
  replacing a chat while its upload is pending.

These are desktop clipboard/drop features. Standalone phone attachment behavior
is not modified or certified by these tests.
