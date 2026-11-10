---
title: Appearance
blurb: Themes, fonts, layouts, and the window's chrome.
---

# Appearance

This page covers how Sheprd looks: themes and light/dark pairs, the window backdrop, fonts, sidebar and tab layouts, status indicators, the title bar, the activity bar, what Herdr plugins add to the sidebar, and the app's language.

Most of these live in **Settings**, whose Appearance group has Theme, Sidebar, Fonts, and Status bar pages. Each setting is saved in `config-gpui.local.toml` (see [Configuration](configuration.md)), and saving that file reloads every open window.

## Themes

**Settings > Theme** shows a large live preview and a searchable grid of theme cards, including every Ghostty theme found on your machine. Clicking a card, or moving through the grid with the arrow keys, applies it at once to every window; there is no Apply button. Only the final choice is saved, when Settings closes or the app quits. The **Ghostty** and **Herdr** filters narrow the grid to one library.

Built-in themes include Default, Nord, Dracula, Catppuccin Mocha, Catppuccin Latte, and popular Ghostty themes such as TokyoNight, Gruvbox Dark, Rose Pine, Atom One Dark, and GitHub Light Default, so they are listed even without Ghostty installed. A theme can also be a Ghostty theme name, an absolute path, or a `~/` path.

```toml
theme = "Nord"
favorite_themes = ["Nord", "Catppuccin Mocha"]
```

The star on a card keeps it in `favorite_themes`, listed above the rest. From a script, use `sheprd theme list` and `sheprd theme set NAME`.

### Following light and dark mode

To follow the system's light or dark appearance, name a theme for each side, as Ghostty does:

```toml
theme = "light:Catppuccin Latte,dark:Catppuccin Mocha"
```

### Other theme sources

- **Follow Herdr** uses the Herdr TUI's shared theme, its palettes, custom colors, and automatic light/dark selection.
- On an Omarchy desktop, `"Omarchy"` follows its current theme as it switches.
- A saved SSH device can paint its terminal with a theme of its own, so you can tell where an agent runs. Choose **Terminal theme…** in the device's right-click menu; **Same as app** removes it. Only the terminal area changes.

### Contrast and accent

The app's own colors (status dots, pull request badges, diff counts, warnings) keep a minimum contrast against the sidebar and menus. **Settings > Theme > High contrast**, or `contrast = "high"`, raises that minimum from 3:1 to 4.5:1 and makes selected rows stand out more. Terminal output always keeps its program's colors.

On Windows, `accent = "system"` takes the app's accent (the current tab, selected rows) from Windows' **Settings > Personalization > Colors**.

## Window backdrop

**Settings > Theme > Window** offers three backdrops:

- **Solid**, the default.
- **Translucent**, with a **Background opacity** stepper. **Blur behind** blurs the desktop on macOS, Windows, and Wayland compositors with KDE's blur protocol. On Windows 11, choose acrylic or **Mica**.
- **Picture**, an image behind the window. Files over 64 MiB are refused.

```toml
[backdrop]
opacity = 1.0
blur = false
# material = "acrylic"
# image = "~/Pictures/wallpaper.jpg"
```

Popups, menus, dialogs, and colors a program chooses stay opaque. On Windows, a window that starts below full opacity shows no browser pages, and switching between the two takes a restart.

## Fonts

**Settings > Fonts** has four roles: Terminal, Sidebar, Tabs, and Interface, each with a family and size. Click a family to choose from installed fonts; **Set all fonts...** changes every family at once. Sizes are logical pixels from 8 to 48. No fonts are bundled.

```toml
[terminal]
# family = "DejaVu Sans Mono"
size = 14
# fallback = ["Symbols Nerd Font Mono"]
# line_height = 1.3
# weight = "medium"
# padding_x = 8
# padding_y = [4, 8]

[sidebar]
size = 12

[tabs]
size = 12

[ui]
size = 12
```

- Defaults are Menlo on macOS, Cascadia Mono on Windows, and DejaVu Sans Mono on Linux; the interface uses the system font on macOS and Windows and DejaVu Sans on Linux.
- Nerd Font icons fall back to installed "Nerd Font" families automatically; `fallback` chooses them yourself. With none installed, a pane showing an icon offers a card naming one to install.
- `padding_x` and `padding_y` keep blank space around the terminal's cells, also under **Settings > Theme > Terminal padding**.
- `cmd-=`, `cmd--`, and `cmd-0` resize the terminal for this session only, and `sheprd font set NAME` changes the family.

## Sidebar layouts

Pick a layout from **View > Layout** or **Settings > Sidebar > Layout preset**. It applies live.

| Layout | Rows |
| --- | --- |
| `normal` | Herdr TUI-like spacing, with branch lines under root workspaces |
| `compact` | The tightest spacing, without branch lines |
| `comfortable` | Roomier padding, branch lines on every workspace, PR change counts |
| `superset` | One line per row, with an icon slot for the PR state or repository owner |
| `orca` | Inset cards with a status column and a meta line |
| `minimal` | Only the status dot and the name, for narrow sidebars |
| `devices` | Agents listed under each device, with a search |

Add `-rounded` to `normal`, `compact`, or `comfortable` for inset rows with rounded corners. New installs start with `comfortable-rounded`.

```toml
layout = "compact"
```

Settings > Sidebar also covers spacing sliders, **Show agents**, **Show search**, **Show repository icons**, **Show device icons**, the device footer, and a color per host (`[sidebar.hosts]`).

### Status indicators

**Settings > Appearance > Indicators** picks how agent status is drawn, with a card previewing each set: **Dots** (the default), **Solid**, **Outline**, **Tiles**, **Wave**, **Braille**, and **Orca**. Working marks always move, and hold still when the system asks to reduce motion. **Edge rail** adds a bar on the leading edge of each working, done, or blocked row.

```toml
status_icons = "dots"
edge_rail = false
```

### Tab layouts

`tab_layout`, or **View > Tab Layout**, sets how tab strips draw: `classic` (the default), `pills`, `outline`, `underline`, `cards`, `curved`, or `compact` (icons only, the chosen tab named).

## Title bar

With Herdr's tab bar at the top (the default), the tab row is the title bar, as in Chrome: the window draws no separate header. The sidebar's first row holds the traffic lights on macOS, the sidebar toggle, and **Back** and **Forward**. The rightmost tab strip ends with the status text, the Git button, your account, and window controls. Every strip keeps empty room after its tabs for dragging the window.

**Back** and **Forward** walk the panes you focused, across tabs and workspaces, like a browser; Cmd-[ and Cmd-] and a mouse's side buttons do the same. A bottom tab bar brings back a full-width header.

On Linux compositors that draw no window frame, such as GNOME on Wayland, Sheprd draws its own minimize, maximize, and close buttons. `show_window_controls` controls them elsewhere on Linux.

## Activity bar

An icon column at the window's left edge picks what the window shows:

- **Workspaces**: the sidebar beside the terminals. Clicking it while shown folds the sidebar to its rail.
- **Overview**: what needs you on every connected host, agents waiting first.
- **Pull requests**, **Issues**, and **Tasks**: see [Issues and pull requests](orchestrator.md).
- **Security**: vulnerability reports and Dependabot alerts of your repositories.
- **Automations**: every host's automation scripts and runs; see [Automations](automations.md).
- **Usage**: every host's plan limits; see [Plan usage](usage.md).

[Mods](mods.md) can add their own icons. A section's icon shows its count in the accent color until you open it. Turn the bar off with **Settings > Sidebar > Show activity bar**, or:

```toml
[sidebar]
activity_bar = false
```

`sheprd ui panel NAME` shows a section from a script.

## Herdr plugins

Herdr runs plugins; Sheprd shows what they report through the daemon. Tab-bar status segments, custom sidebar tokens, and agent views appear as the daemon reports them. For example, [Usage Tracker](https://github.com/VHemanth45/herdr_agents_tracker) adds account usage percentages to the header and context meters to the sidebar.

**Settings > Plugins** lists every custom `$name` value plugins report on a connected host, with an example and the hosts reporting it. For an agent in a Herdr 2 pane, the labels a plugin or hook sets on its herdr-core terminal are its custom values, under the label's name; the labels Sheprd sets itself (`acp` and `sheprd.*`) are left out. Each has a switch that adds or removes it as a row in Herdr's shared `[ui.sidebar]` config, so the terminal client shows the same rows, and a preview draws them. On Windows the switches are read-only.

Two `[usage]` switches hide plugin output:

```toml
[usage]
topbar = true   # Daemon toolbar text, including plugin percentages
inline = true   # Daemon sidebar rows, styles, and custom tokens
```

Plugin actions bound in Herdr's config with `[[keys.command]] type = "plugin_action"` run from their key and the command palette. Sheprd cannot list, enable, or disable plugins.

## Language

The `language` setting is `system` (the default, following the OS's preferred languages) or a language tag such as `fr-CA`. **Settings > General > Language** picks it, and `sheprd language set TAG` writes it.

> **Note.** Only English ships so far, so any other language shows English. Terminal output, names, and what the daemon reports never change with it.
