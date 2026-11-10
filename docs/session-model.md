# Session model

Sheprd supports backends that disagree about what a session is. Herdr 1 owns
workspaces, tabs, panes, and agents. herdr-core owns terminals only. The goal
is one model the app reads whichever backend runs, so a feature stops depending
on Herdr 1's snapshot and a new backend costs an adapter rather than a rewrite.

## Where it stands

| Layer | Herdr 1 | herdr-core |
| --- | --- | --- |
| Terminals (`sheprd-backend`) | Not implemented; `sheprd-client` talks to the daemon | `CoreBackend` |
| Tab and pane layout | The daemon's snapshot | `sheprd_session::panes`, saved with the tab |
| Workspaces | The daemon's snapshot | Kept by the app beside its tabs (`browser-tabs.json`'s `workspaces`, read by `SavedTabs::workspaces`), under the scope of the host whose core runs them (this computer, or a saved SSH, cloud, or WSL host), listed after Herdr 1's; with no Herdr 1 installed, the first is `core_terminals::WORKSPACE`. A terminal started outside the app for a workspace, as by a phone, carries `sheprd_session::WORKSPACE_LABEL` and gets a tab there (`core_terminals::adopt`) |
| Agents | The daemon's snapshot | `TerminalInfo::agent`, per terminal; which agent by `sheprd_core_client::agent_kind` |
| Commands | 43 `sheprd_client::Method` variants | Handled per feature in `core_terminals` |
| `Session` | `session_model::of_snapshot`, cached per snapshot in `LiveState::session` | `Session::core`, rebuilt on the window tick from the app's tabs and the core's terminals |

`sheprd-session` holds what is already shared: the pane tree, the reader of the
app's saved tabs, which the remote control daemon used to parse as untyped JSON,
and `Session`: workspaces, their tabs, and agents, each workspace marked with the
`Keeper` that carries out what is asked of it. A test in the app reads what the
app saves through the saved tabs reader, so the two cannot drift apart.

`SheprdWindow::sessions` gives a host's sessions together: Herdr 1's, and the
one over herdr-core, on this computer and on each saved host whose workspaces
the app keeps. Readers walk both, so a reader moved onto
it lists Herdr 2 work beside Herdr 1's.

| Reader | On `Session` |
| --- | --- |
| Devices overview: counts, agent lines, search | Yes |
| Sidebar workspace rows, rail, search, reorder, previous and next workspace | Yes |
| Window title | Yes |
| Tab strip | Herdr 1's tabs only while its workspace is in front (`herdr_workspace_in_front`) |
| Palette Go To and jump list | Kept workspaces added beside Herdr 1's rows |
| Workspace menu | Kept workspaces: rename, close, a note once Git names the checkout, and new and deleted worktrees, which the app makes with Git (`sheprd_remote::work`) |
| Git status, pull request prefetch, worktree rule, notes | Kept workspaces whose folder Git names (`core_terminals::checkouts`) |
| Agent checkpoints | Yes: `sheprd_vcs::checkpoint::Checkpoints::observe` takes a `Session`; Herdr 2's is watched as its own endpoint |
| Mods state: workspaces, agents, focus | Yes |

herdr-core reports no Git facts, so the app asks Git for each kept workspace's
folder on a worker (`sheprd_vcs::git::identity`): its branch, its repository's
common directory as the worktree key, and whether it is a linked worktree.

The app still reads `ClientShell*` types directly in about 67 files. That is the
work left.

## Moving the app onto it

Move one reader at a time, in separate pull requests, so parallel work conflicts
as little as possible:

1. Add `Session` to `sheprd-session`: workspaces, tabs, and agents as domain
   types, built from a `ClientShellSnapshot` in the app (`session_model`)
   rather than through a new dependency here; a `From` impl would need one,
   as both types are foreign to the app. It is a model, not a mirror of the wire types: only what
   features read, and nothing the daemon's protocol alone needs.
2. Keep it next to `LiveState::snapshot`, built once when a snapshot lands, so
   render paths stay thin and revision fencing stays where it is.
3. Move readers that only list things first: the overview, then the sidebar's
   workspace rows, the tab strip, and the palette.
4. Move commands last. Each becomes an intent the backend carries out or
   refuses, so a backend without the capability says so instead of failing late.
5. Build a `Session` from herdr-core: several workspaces, saved by the app. This
   replaces `core_terminals::WORKSPACE` and is what the default flip in the
   Herdr 2 rollout waits on. Done: a new workspace is the app's with Herdr 2
   chosen for new terminals or with no Herdr 1 installed. A kept workspace in
   front (`SheprdWindow::kept_in_front`) is the window's workspace whatever
   Herdr 1 focuses, and choosing anything of Herdr 1's hands the front back.

## Decisions

- Layout stays in the app's saved tabs rather than a core document. The app
  saves the tabs of every core it reaches, on this computer or over SSH, and
  the remote control daemon reads the same file,
  so a document would have no reader the file does not already serve. Revisit
  when a second client on another machine needs a core's layout.
- Sheprd's saved tabs win on the phone. A `herdr-app` document lists what they
  do not hold, so a terminal in both stays in Sheprd's tab and its split.
- No daemon of our own. Add one only for something that must run while the app
  is closed and that neither `doc.*` nor `sheprd-automate` can hold.
