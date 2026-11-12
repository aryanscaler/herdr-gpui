# Phone layout ownership

iOS (`RemoteClient.action`, `rename`, and workspace rename) and the web
client use the same authenticated routes: `/api/term/{pane}/action`,
`/api/term/{pane}/rename`, and `/api/v2/workspaces/{workspace}/rename`.
Their handlers live in `sheprd-remote-control/src/web/terminal.rs`.

For Herdr 1 these call its session API. For Herdr 2, `core_keeper` identifies
the owner and `native::changes::arrange` routes the intent:

| Owner | Split, zoom and names | Persistence |
| --- | --- | --- |
| Sheprd saved tab | Daemon edits while the saved-tabs lock is free; otherwise the desktop answers through `control::layout` | `browser-tabs.json`, read under the same lock when desktop opens |
| Daemon-owned tab | Daemon edits directly, including on a directly paired SSH/cloud host | `core-tabs.json`, scoped to the core boot identity |
| External Herdr desktop `herdr-app` tab | Explicitly refused; split/zoom/rename capabilities are omitted | Owned by the unrelated app; there is no supported mutation contract |

A recent desktop heartbeat is not ownership. It can outlive the desktop by
twelve seconds. The daemon acquires the saved-tabs lock before it edits or
starts a split shell, and queues only when that lock is held. If the desktop
exits while an intent is still queued, the daemon withdraws it and applies it
under the lock. An intent already taken by the desktop is **not replayed**:
its answer may have been lost after the change was saved. The phone receives
an error promptly and should refresh before deciding what to do next.

Desktop startup must also acquire this lock before reading or starting its
saved-tabs writer. If a slow phone operation still holds it after the two-second
startup budget, startup reports an ownership error and exits; opening Sheprd
again after the operation finishes reads the complete updated layout. It never
continues with an unlocked snapshot that could overwrite the phone's split.

Saved-file edits serialize concurrent phones, preserve unrelated fields, and
clean up a shell if recording its split fails. Splitting selects the new pane
and clears zoom; zooming selects its pane. Saved and daemon-owned selections
are projected without reordering the pane tree. Names, selection, and zoom
survive reading the corresponding file again.

## Verification boundaries

Deterministic regressions cover stale/absent desktop routing, exit before and
after an intent is taken, concurrent saved edits, saved-file reopening,
daemon-owned restart/cleanup, selection projection, and failed-split cleanup.
The shared endpoints do not depend on a desktop relay when paired directly
to another host. Live iOS/web operation on SSH/cloud machines remains runtime
QA; mock tests are not evidence that native input or pairing was exercised.

Desktop adoption of formerly daemon-owned terminals is a separate transition:
`core_terminals/adopt.rs` currently creates single-pane tabs for labelled
terminals, and saved tabs then take precedence over daemon-owned groups.
This is not proof that a daemon-owned split is imported intact into the
desktop. A cross-client transfer contract is needed before claiming that
transition preserves grouping and names, especially across hosts. Saved-tab
reopening does preserve the file edited by the phone. Updating the desktop
while it is closed is an independent capability limit.
