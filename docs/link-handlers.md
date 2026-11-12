# Herdr 2 link-handler adapters

Sheprd can run explicitly configured commands for **link-modifier clicks in
Herdr 2 desktop panes**. Nothing runs on hover, plain clicks, selection, or
terminal output alone. Herdr 1 continues to use its daemon's plugin handlers.

Add adapters to `config-gpui.local.toml`. This is trusted executable
configuration: select the command or plugin directory yourself, rather than copying commands from
terminal output. No Herdr plugin registry, manifest, startup hook, or build hook
is automatically imported.

## Existing manifests (opt-in)

To use the link actions from an existing **trusted, installed** plugin, opt in
to its directory on the pane's host:

```toml
[[link_plugins]]
host = "local" # or the exact remote ID from sheprd host list --json
directory = "/absolute/path/to/my-plugin"
enabled = true
# Optional existing plugin-owned directories:
# config_directory = "/absolute/path/to/plugin-config"
# state_directory = "/absolute/path/to/plugin-state"
```

Only `herdr-plugin.toml` in that directory is read, on a background worker
after a click. Only `[[link_handlers]]` and their referenced `[[actions]]`
commands are adapted. The manifest's plugin ID determines ordering, and its
handler declaration order is retained. The action and handler independently
inherit the plugin's `platforms` unless they explicitly override it; both
must support the execution host. Local platform detection is native, while a
remote host is probed with `uname -s` through its own transport. Unsupported
handlers are skipped before choosing the first match. Disabled or other-host
entries are not read. A malformed/missing manifest reports an error without
running a command or fallback. At most 16 manifest entries, 256 KiB per
manifest, and 64 combined handlers are accepted.

This opt-in list is app-owned, independent of Herdr 1's registry. Set
`enabled = false` to disable an entry here; changes to Herdr 1's registry do
not silently enable or disable it. Manifest edits apply to subsequent clicks.
Install/build the plugin yourself first: build, startup and event hooks are
never run by a click. A command that requires Herdr 1 APIs still needs adaptation
as described below.

## Explicit command adapters

```toml
[[link_handlers]]
plugin = "example.notes"
id = "markdown"
host = "local"
pattern = '^file://.*\.md$'
directory = "/absolute/path/to/my-plugin"
command = ["/absolute/path/to/my-plugin/open-link"]
action = "open-link" # original manifest action ID, when needed
# Optional existing directories on this host, owned/configured by the user:
# config_directory = "/absolute/path/to/plugin-config"
# state_directory = "/absolute/path/to/plugin-state"
enabled = true
# Optional platform filter; omitted means every platform:
# platforms = ["linux", "macos", "windows"]
```

`command` is an argv array, not shell text. A shell is used only if explicitly
named in that array. The URL is never substituted into argv or shell code.
`directory` is the trusted command directory on the target machine, not a cwd
provided by terminal output. Use absolute paths for both it and the executable.

Handlers match the original OSC 8 hyperlink target, including `file://` links
on remote hosts, or a printed web URL. Printed file paths do not invoke handlers.
Enabled handlers are sorted by `plugin` lexically, then retain their declaration
order within that plugin, as Herdr 1 does. The first matching regex claims the
click. Exit failure, launch failure, cancellation, or connection failure never
tries another handler or opens the fallback as well. With no match, normal
browser, file, external-scheme confirmation, and Alt target selection apply.
An unclaimed remote file is never opened from the local disk.

`host` is required: `local` for this computer, otherwise the exact saved device
scope. There is no wildcard or remote-to-local fallback. Each remote adapter's
command and directory must exist on that host. SSH, cloud and WSL use the
existing host transport; native local commands use argv on Windows too. A
changed or removed device refuses activation from its old view. Host selection
cannot redirect an already accepted click. One handler runs per window at a
time; repeated clicks while it runs are consumed. Output is bounded to 64 KiB
and runtime to 30 seconds; closing the owning window cancels its worker.

## Command contract and compatibility limits

Commands receive `HERDR_PLUGIN_ID`, `HERDR_PLUGIN_LINK_HANDLER_ID`,
`HERDR_PLUGIN_CLICKED_URL`, and `HERDR_PLUGIN_CONTEXT_JSON` containing
`invocation_source = "link_click"`, `clicked_url`, `link_handler_id`, and the
terminal's reported `focused_pane_cwd` (data only; never the process cwd).
`HERDR_PLUGIN_ROOT` is `directory`. Optional `action`, `config_directory`, and
`state_directory` supply `HERDR_PLUGIN_ACTION_ID`, `HERDR_PLUGIN_CONFIG_DIR`,
and `HERDR_PLUGIN_STATE_DIR`. Provision these directories on the named host
first; omitted values are cleared rather than inherited from another plugin.
`SHEPRD_LINK_TERMINAL_ID` and `SHEPRD_LINK_HOST` identify the clicked core
terminal and its host. They are data, not Herdr 1 pane IDs. Inherited pane and
API socket routing is cleared.

This supports standalone commands or purpose-built adapters. It is **not full
Herdr plugin runtime compatibility**: plugins requiring `HERDR_SOCKET_PATH`,
Herdr 1 workspace/tab/pane IDs, plugin popups, lifecycle hooks, installation
leases, or daemon APIs need adaptation. No fake Herdr 1 identity is supplied.
Plugin opt-in and enable state are explicit through per-host configuration;
manifest platform inheritance is honored by `link_plugins`. Phone/web clicks, including with
the desktop closed, do not invoke these desktop adapters. No Herdr 1 custom
link activation route was found in the current phone/web client either, so
this is a shared capability gap, not a demonstrated Herdr 2 regression.

### Capability audit and ownership

| Plugin requirement | Status / next owner |
| --- | --- |
| Match URL and run a standalone argv action in plugin root | This slice supports it, local and explicit remote host routes |
| Read clicked URL, action/handler IDs, root/config/state paths, pane cwd | This slice supplies those data fields without inventing H1 identity |
| Read existing link actions with platform inheritance and enable/disable | This slice supports explicit `link_plugins` directories and app-owned enable state. It does not silently read Herdr 1 registry state |
| Open a core split/tab or app browser through a plugin-specific command | App-owned adapter can use existing Sheprd CLI capabilities, but the existing H1 `plugin.pane.open` socket protocol is not aliased; broader plugin-runtime owner should define the explicit H2 contract |
| Call H1 `pane.*`, `workspace.*`, `plugin.pane.open` using `HERDR_SOCKET_PATH` and H1 pane IDs | Requires adapting the plugin to Sheprd/core APIs or a deliberately implemented compatibility service. H1 cannot address a core terminal; passing a core ID to H1 is incorrect. This is unimplemented app/runtime compatibility, not evidence that upstream Herdr must change |
| Startup/build/event hooks, plugin installation leases, persistent logs | Broader app plugin-runtime/registry ownership; not necessary for standalone click actions, and not run implicitly by this slice |
| Phone/web custom link activation | Shared remote-control capability work; no current H1 activation route found |

Reference evidence: Herdr `app/api/plugins/mod.rs` matches sorted plugin IDs
and declaration-order handlers, then constructs a `link_click` context;
`app/api/plugins/runtime.rs` exports the daemon socket and pane IDs;
`app/api/plugins/env.rs` supplies root/config/state directories. Those are
distinct requirements, not a blanket inability to support plugins in Sheprd.
In this repository, phone/web terminal operations are in
`crates/sheprd-remote-control/src/native` and `src/web`; neither exposes
`pane.link.activate` or a plugin-action dispatch route. iOS terminal input in
`ios/SheprdRemote/Views/TerminalScreen.swift` does not dispatch that method.

For existing plugins, inspect their action command and dependencies first. Copy
only a compatible link action into an adapter, preserving plugin IDs and handler
order; install its dependencies on the named host. Do not copy startup/build
commands. Transparent compatibility needs a separately specified host-side
plugin runtime/API contract; it is not achieved by sending a core terminal ID
to a Herdr 1 daemon.

## CLI

The existing configuration CLI reads, validates, replaces, and removes the array:

```sh
sheprd config get link_handlers --json
sheprd config check --json
sheprd config set link_handlers '[]' --json
sheprd config unset link_handlers --json
sheprd config get link_plugins --json
sheprd config set link_plugins '[]' --json
```

There is deliberately no handler-execution CLI: activation requires a terminal
link click. Configuration reload applies to subsequent clicks.
