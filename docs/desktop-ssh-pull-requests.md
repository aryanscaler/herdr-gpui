# Desktop pull requests for Herdr 2 SSH workspaces

The desktop reads pull requests for app-kept workspaces and the shown Herdr 2
checkout on a saved SSH host without requiring a connected Herdr 1 daemon or
snapshot. Git names the branch and repository on that host; the existing
`sheprd-github::pull_request::Origin::Ssh` transport resolves the checkout there.

The selected device's GitHub account and configured additional-account routing
are used, following the same policy as Herdr 1. If the device has no account,
the existing main-account fallback still applies. A device-specific sign-in
works with the main account signed out. The local Git status/commit/push worker
does not receive SSH checkout paths.

Background prefetch supplies the sidebar badges, workspace PR menu, and merged
checkout discovery. Opening the workspace menu presents the cached answer.
Cache answers are scoped to SSH origin and account set; changing selection,
connection generation, account, or token retires in-flight requests. A menu
also checks that its workspace still names the same directory, repository and
branch. Merged checkout discovery explicitly reads the selected device's view,
so a previous device's cached PR cannot authorize cleanup on another host.

## Scope and dependencies

- Host-scoped checkout metadata and remote cleanup execution come from the
  remote-worktrees foundation, PR #1286.
- Cloud and WSL desktop PR discovery remain unsupported: `Origin` supports
  Local and SSH only. This is a pre-existing limitation of **both** terminal
  backends, not an H2-only regression. Remote worktree operations and cleanup
  routing do not themselves provide cloud/WSL PR discovery.
- Standalone phone GitHub readers are owned separately by PR #1282.
- Existing desktop PR commands and confirmation policies are unchanged; no new
  transport or CLI operation is introduced.

## Verification

Deterministic tests cover SSH-only kept-workspace prefetch/menu presentation,
device-only sign-in, identical local/remote paths, shown-checkout host fencing,
wrong-host/account cache isolation, sign-out, and late responses after changes
of SSH host, account, or connection generation. Native SSH UI and actual remote
GitHub access still require runtime QA; headless fixtures do not prove those.
