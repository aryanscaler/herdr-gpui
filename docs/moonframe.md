# Sheprd on Moonframe

`just run-moonframe` starts Sheprd in a window [Moonframe](https://moonframe.dev)
lays out. It is the `moonframe` feature of `sheprd-gpui`, off by default, so the
app `just run` starts is unchanged. A build with it opens this window on a plain
launch; `sheprd` commands and `--socket` or `--session` launches stay the app's
own. Quit Sheprd first, as both use your settings and daemon. CI builds and
tests the feature (the all-features legs) and loads its Luau with `--check`.

Everything around the terminals is Luau in `crates/sheprd-gpui/moonframe/app`:

| File | Draws |
| --- | --- |
| `main.luau` | The layout: where each part goes |
| `mods/header/main.luau` | Back, Forward, and the sidebar toggle |
| `mods/activity/main.luau` | The activity bar |
| `mods/worktrees/main.luau` | The left pane's head and worktree rows, linked worktrees under their checkout, drawn in the sidebar layout Sheprd's config names (`layout = "orca"`, `compact`, `minimal`, ...) |
| `mods/agents/main.luau` | The agents under the worktrees, in Sheprd's sort and the same layout |
| `mods/details/main.luau` | The right pane; open it from Details in the status bar |
| `mods/status/main.luau` | The status bar |
| `components/sheprd-*.luau` | What the worktrees and agents share: a row highlighted as the layout marks the selected one, the status dot, and the section names |
| `style.luau` | The hover looks, for every mod |

The terminals, tabs, and pages stay one Rust view, placed with
`ui.view { name = "sheprd", id = "main", props = { region = "main" } }`.

## Changing it

Saving a file reloads the window. To keep a change of your own, copy the file
to the same path under `~/.config/sheprd-moonframe` and edit the copy; deleting
it brings the original back. The sparkles at the bottom right ask an agent to
make a change, and it asks before each edit. View > Inspect Element (⌥⌘I, or
⇧⌘X to go straight to picking) shows what built any part, and its file and line.

Run `just run-moonframe --check` after a change. It loads what is
drawn at that moment, so a mistake in a row that only appears with an agent
shows in the log when one appears: `unknown key` names it.

## `app.sheprd`

| Field | Holds |
| --- | --- |
| `devices` | Each device the sidebar lists, in its order: `id`, `label`, `selected`, `connected`, `enabled`, `removing`, `collapsed`, `load` (`cpu`, `memory`), `search` (every workspace, folded or not, with `label`, `branch` and `note`, for the sidebar's search), and `workspaces`, its visible rows (fields as below, plus `endpoint`, `group` and `collapsed` on a checkout that folds its worktrees, `note`, `muted`, `teleported`) |
| `multi` | Whether several devices are listed, so each shows a header |
| `build` | The worktree a build came from: `worktree`, `branch`, `pr` |
| `workspaces` | The selected device's, folded or not, in the sidebar's order: `id`, `number`, `label`, `branch`, `repo`, `repo_key`, `linked`, `child` (shown under its checkout), `focused`, `agent_status`, `ahead`, `behind`, `added`, `removed`, `dirty`, `pr`, `pr_color` (the number's color for its state) |
| `agents` | In Sheprd's sort: `name`, `kind` (`claude`, `codex`, ...), `status`, `state` (the status's word, when the agent's integration shows one), `title`, `pane`, `pane_name`, `workspace`, `workspace_label`, `tab`, `focused` |
| `agent_sort` | `grouped` or `priority` |
| `layout` | The configured sidebar layout's `name` and spacing: `padding`, `gap`, `row_padding`, `style_padding`, `child_indent`, `inset`, `spacing`, `radius`, `outline` (rounded highlights), `tree_lines`, `title_case`, `workspace_details`, `child_details`, `pr_counts` |
| `panels`, `panel` | The activity panels (`id`, `label`, `count`, `unseen`) and the current one's `id` |
| `connected`, `status` | Whether the daemon answers, and what to say about it |
| `section`, `sidebar_visible`, `status_bar_visible` | What the window shows |

Two parts change on their own, so they have their own providers and a change
runs only the views that read them:

| Provider | Holds |
| --- | --- |
| `app.sheprd_load` | `cpu` and `memory`, as percentages |
| `app.sheprd_theme` | `accent`, `active`, `added` and `removed` (change counts), `background`, `danger`, `dirty` and `dirty_ink` (the uncommitted-work chip), `foreground`, `muted`, `on_accent` and `on_muted` (text over those), `sidebar`, `status` (each agent status's color), `subtext`, `success`, `surface`, `warning` |

Moonframe's own widgets follow Sheprd's theme and UI font without reading these.
`font = "mono"` is Sheprd's sidebar font.

## Menus

The menu bar is Sheprd's own, as its window has it (Sheprd, File, Edit, View,
Terminal, Window), with Moonframe's Developer menu before Window: Developer
Mode and, while it is on, Components, Reload Modules, Undo and Redo, Inspect
Element, Developer Tools and Slow Animations. A menu item reaches Sheprd
wherever the focus is, in the Luau layout or a terminal.

## Asking Sheprd for something

Send a message with `to = "provider:sheprd"`:

| Message | Does |
| --- | --- |
| `{ command = "name" }` | Runs any command by its name in the catalog, such as `new_workspace`, `toggle_sidebar`, or `themes` |
| `{ focus_workspace = id }` | Focuses a workspace |
| `{ focus_pane = id }` | Focuses a pane |
| `{ panel = id }` | Shows an activity panel |
| `{ menu = "workspace", workspace = id }` | Opens Sheprd's own popover for a workspace at the pointer, without selecting it: its details, actions (close, delete the worktree, rename, mute, note, teleport, ...) and pull request. Send it as a row's `context`, so a right click opens it |
| `{ menu = "agent", pane = id }` | Opens an agent's pane menu at the pointer |
| `{ menu = "device", endpoint = id }` | Opens a device's menu at the pointer |
| `{ note = id }` | Edits the note on a workspace's checkout |
| `{ toggle_repository = key }` | Folds or unfolds a repository's linked worktrees |
| `{ fold_all = true }` | Folds every repository's worktrees on every device; `false` unfolds them |
| `{ toggle_device = id }`, `{ select_device = id }` | Folds a device's header, or selects the device |
| `{ toggle_agent_sort = true }` | Switches the agents between grouped and by priority |
| `{ move_workspace = id, target = id, after = bool }` | Moves a workspace of the selected device before (or after) another, as dropping its row there does; the worktrees mod sends it when a row with `carry` is dropped on one with `accept` |
| `{ sidebar_width = true }` as a `pointer` | Resizes the sidebar from a drag of its edge (`phase`, `dx`); a double click puts back the default width. The shell draws the edge as a hairline lit on hover (`.sheprd-left-edge` in `style.luau`) |
| `{ open_build_pr = true }` | Opens the pull request a worktree build came from |

A row on another device adds `endpoint = id` to its message; without it the
selected device is meant.

## What is not there yet

The window starts as Sheprd's own does (`app::install_services`): it answers
the `sheprd` command line, sends system notifications, follows links, checks
for updates, badges the Dock, and writes Sheprd's log and crash reports. What
looks for a main window finds it through `window::main_window`, whatever the
window's root view.

The window reopens where it was and as large, as Moonframe keeps it
(`window = { remember = false }` in `app/main.luau` turns that off).

Still missing: New Window opens a window of Sheprd's own rather than another
Luau one, until Moonframe can open a second main window. The sidebar has no
device tag sections, no Devices layout or agents nested under their
workspaces, and no menu that opens as the pointer rests on a row.

## Measuring

Compare release builds: a debug window shows a yellow `dev build` strip, and
runs several times slower. GPUI times every frame in either app, so the same
line reads both:

```sh
ZED_MEASUREMENTS=1 RUST_LOG=moonframe::perf=debug target/moonframe/release/sheprd
ZED_MEASUREMENTS=1 target/release/sheprd
```

`frame duration` is one frame's draw and present. `moonframe::perf` adds what
each publish and message ran. Machine load moves these a lot, so alternate the
two apps and compare medians.

`just bench-moonframe /path/to/herdr` does that for you: it builds both apps,
runs each in turn on a private daemon with 14 linked worktrees, through idle,
terminal output, agents changing state, and focusing each workspace, and
prints the medians of three rounds. It opens windows, so it needs a desktop.
`scripts/bench_frames.py` takes other binaries, round counts, and `--json`.
