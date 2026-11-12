---
title: Configuration
blurb: Where settings live and how to change them.
---

# Configuration

Sheprd keeps its own settings in TOML files next to Herdr's, and shares a few with Herdr's terminal client. This page explains which file holds what, how the Settings window relates to the files, and how changes are applied.

## Where the files are

Settings live in `$XDG_CONFIG_HOME/herdr/`, which is `~/.config/herdr/` by default and `%APPDATA%\herdr\` on Windows. There are three files that matter:

| File | What it is |
| --- | --- |
| `config-gpui.toml` | Sheprd's managed defaults, with comments documenting each setting. Its first line says **DO NOT EDIT -- WILL BE OVERWRITTEN**: every start and reload replaces it with the current release's defaults, so new settings show up there. |
| `config-gpui.local.toml` | Your Sheprd overrides. Edit this one. The Settings window and the theme picker save here too. |
| `config.toml` | Herdr's shared config, read by both Herdr's terminal client and Sheprd for the settings they share. |

The files are created for you. To print their paths, run:

```sh
sheprd config path
```

An app opened from Finder or a desktop launcher does not inherit your shell's `XDG_CONFIG_HOME`. When it has none, Sheprd asks your login shell for it once at startup, and uses the result for its settings, Herdr's `config.toml` and the daemon's sockets.

## How overrides merge

`config-gpui.local.toml` only needs the keys you change. Everything you leave out follows the managed defaults:

- Nested tables merge key by key.
- Arrays replace the default rather than append to it.
- To go back to a default, delete the key from your local file.

Keys this build does not know, such as ones a newer build reads, are ignored and named in a dismissible warning. A local file with invalid settings does not break the app: it keeps the last valid configuration and shows a load error.

For every key and its default, read the managed file or the [example config on GitHub](https://github.com/sheprd-labs/sheprd/blob/main/crates/sheprd-gpui/config-gpui.example.toml).

## The Settings window

Open Settings with Cmd-, on macOS or Ctrl+Shift+, on Linux and Windows. It opens in its own window, and the terminal stays usable while it is open. Its pages are grouped:

- **Appearance:** Theme, Sidebar, Fonts, Status bar, Chat
- **Behaviour:** General, Tabs & panes, Code, Notifications
- **Connect:** Integrations, Plugins, Cloud devices
- **Advanced:** Configuration

A search box above them finds any row. **General** lists every optional `[features]` flag with a switch and a description.

Each row is tagged with the app that reads it. **Sheprd** rows are saved in `config-gpui.local.toml`. **Herdr** rows are saved in Herdr's shared `config.toml`, so Herdr's terminal client follows them too. Saves happen automatically, keep your comments and unrelated keys, and never block the window. Theme and sidebar layout choices are the exception: you can browse them live, and only the final choice is saved when Settings closes.

## Live reload

Saving `config-gpui.local.toml` reloads every open Sheprd window, usually within half a second. Font, size, theme and layout changes apply together. Changes to Herdr's shared `config.toml` are picked up as well.

You can also change settings from a terminal. `sheprd config` edits `config-gpui.local.toml` while keeping its comments, refuses a value that would not load, and works whether or not the app is running:

```sh
sheprd config get focus_follows_mouse
sheprd config set focus_follows_mouse false
sheprd config unset focus_follows_mouse
sheprd config check
sheprd config reload
```

`config check` reports invalid settings, and `config reload` rereads both Herdr's config and Sheprd's. See [Command line](cli.md).

## Settings shared with Herdr

Theme, status indicator style, sound and toast delivery are shared with Herdr's terminal client and read from Herdr's `config.toml` (or the file `HERDR_CONFIG_PATH` names). Sheprd's own file can override them for this app only.

These are preferences on this computer. Selecting another device does not change which settings apply:

| Scope | Settings | Source |
| --- | --- | --- |
| This app | Theme, indicator style, sound, toast delivery, sidebar and input preferences, and `[keys]` | Local `config.toml`, under your `config-gpui.toml` overrides |
| Each host | Worktree directory, custom commands, pane defaults, pane borders, gaps and scrollbars, and integrations | That host's daemon |

Pane chrome in Herdr's `[ui]` table, such as `pane_borders` and `pane_gaps`, is drawn by the daemon, so a remote device shows what its own `config.toml` chose. Sheprd never reads a remote host's `config.toml`.

> **Windows.** Shared Herdr settings are read-only on Windows. Sheprd's own settings stay editable.

On macOS and Linux, Sheprd refuses a config file or folder that other users can write to, and names the path in the error. Remove the group and other write permission, then click **Reload** in Settings:

```sh
chmod go-w ~/.config/herdr ~/.config/herdr/config.toml
```

## Examples

Pick a theme, or a pair that follows the system's light and dark appearance:

```toml
theme = "light:Catppuccin Latte,dark:Catppuccin Mocha"
```

Set the terminal font and padding. Font sizes are logical pixels from 8 to 48:

```toml
[terminal]
family = "DejaVu Sans Mono"
size = 14
padding_x = 8        # left and right
padding_y = [4, 8]   # top, then bottom
```

Choose a sidebar layout. This is a top-level key, so put it before any table header:

```toml
layout = "compact"
```

Rebind shortcuts. An empty list unbinds a command:

```toml
[keybindings]
new_tab = "ctrl-shift-t"
split_right = ["ctrl-shift-d", "alt-d"]
close_tab = [] # unbind; remove this line to restore inherited defaults
```

Tune in-app notifications:

```toml
[notifications]
enabled = false
delay_seconds = 1
position = "bottom-right"
finished = true
```

Opt in to beta releases:

```toml
[updates]
channel = "beta"
```

Related pages cover each area in depth: [Appearance](appearance.md), [Keyboard](keyboard.md), [Notifications](notifications.md), [Usage](usage.md) and [Updates](updates.md).
