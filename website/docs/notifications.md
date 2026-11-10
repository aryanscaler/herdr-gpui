---
title: Notifications
blurb: Toasts, system notifications, sounds and badges.
status: shipped
platforms: macOS, Linux, Windows
---

# Notifications

Sheprd tells you when an agent finishes or needs you: in-app toasts or system notifications, sounds, a Dock or taskbar badge, and status dots. This page covers each one, how to quiet a device, workspace, or tab, and the settings involved.

## Status dots

Every tab, sidebar row, and agent shows a status dot: idle, working, blocked (waiting for you), or done. The dots come from the Herdr daemon's own status, so Sheprd and Herdr's terminal client always show the same dot, and every Sheprd window agrees. Opening a pane does not clear a dot on its own; finished agents clear as the daemon marks them seen.

How the dots are drawn (dots, tiles, braille, and other sets) is covered in [Appearance](appearance.md#status-indicators).

## Toasts and system notifications

Notifications fire when an agent finishes, when one needs your attention, and for custom and update notices. Where they go follows Herdr's shared `[ui.toast]` setting in `~/.config/herdr/config.toml`, so the Herdr TUI and Sheprd agree:

- `delivery = "herdr"` shows in-app toasts.
- `delivery = "system"` posts them to the OS notification center instead.
- `"off"` and `"terminal"` show neither; Sheprd has no outer terminal to hand them to.

**Settings > Notifications** edits delivery, the delay, and **Finished notifications**. To override the shared setting for Sheprd alone, use `[notifications]` in `config-gpui.local.toml`; each key overrides on its own:

```toml
[notifications]
enabled = false
delay_seconds = 1
position = "bottom-right"
finished = true
```

- `enabled = true` shows in-app toasts even when shared delivery is `system`.
- `delay_seconds` is 0 to 3600 (default 1). Custom notifications are always immediate.
- `position` is `top-left`, `top-right`, `bottom-left`, or `bottom-right`.
- `finished = false` stops "finished" toasts and OS notifications while keeping "needs attention" and custom ones, for example when an agent hook posts its own summary with `herdr notification show`. Sounds are not affected.

```sh
sheprd config set notifications.finished false
```

A notification for the active tab of the selected host is skipped only while the window is focused; background tabs, other hosts, and unfocused windows always post. With several windows on one host, an event posts once. When the window has more than one host, the first line names the host.

Clicking a notification brings the app forward and opens the pane, tab, or workspace it is about. Clicks are handled only while Sheprd runs.

### Per platform

- **macOS.** The first notification asks for permission, and macOS shows that prompt instead of the notification. If you deny it, allow notifications again in **System Settings > Notifications**. Banners also show while Sheprd is in front, for events outside the active tab.
- **Linux.** Uses the freedesktop notification service; without a notification daemon nothing appears. Clicking needs a server that supports the default action. Notifications are not replaced, so each event shows separately.
- **Windows.** Uses Windows toasts. With the installer's link registration, clicking a toast, even later in the notification center, opens its pane; a click after the app quit only starts it.

System notifications are silent: sounds follow the sound settings below.

## Muting a device, workspace, or tab

Right-click to quiet one place instead of everything:

- a device's sidebar header has **Mute notifications**, Local included;
- the workspace menu's header has **Mute** (**Unmute** while muted);
- the tab menu has **Mute Notifications**.

From a pane, `sheprd host mute` and `sheprd worktree mute` do the same (`unmute` lifts them).

Nothing from a muted place asks for you: no toast, no OS notification, no sound, and its agents leave the Dock badge, the Windows taskbar overlay, and the Overview count. Status dots still show what its agents do. A struck-through bell marks what is muted. Notifications dropped while muted are not replayed when you unmute.

A workspace or tab mute does not hold back `herdr notification show` messages or terminal bells, which Herdr sends without saying where they came from, nor [phone notifications](remote-control.md). A device mute does hold back the first two.

Herdr 2 tabs and the workspaces Sheprd keeps for them mute the same way, from the tab menu and the workspace menu. A muted Herdr 2 tab also rings no terminal bell, since Sheprd hears its bells itself, and a Herdr 2 tab in a Herdr 1 workspace follows that workspace's mute.

Mutes stay in Sheprd, survive restarts and renames, and are forgotten when the workspace or tab closes. A device's mute holds while it is offline, until you lift it or remove the device.

## Notification sounds

Sounds play when an agent finishes (Done) or needs you (Request). They share the Herdr TUI's configuration in `~/.config/herdr/config.toml` (or `$XDG_CONFIG_HOME/herdr/config.toml`, or the file `HERDR_CONFIG_PATH` names), not a remote host's:

```toml
[ui.sound]
enabled = true
# path = "sounds/all.mp3"
# done_path = "sounds/done.mp3"
# request_path = "sounds/request.mp3"

[ui.sound.agents]
droid = "off"
# claude = "off"
# open_code = "on"

[ui.toast]
delay_seconds = 1
```

- Sound is on by default; Droid alone defaults to off.
- Agent values are `default`, `on`, or `off`; the global `enabled` switch wins.
- Per-sound paths override `path`, and relative paths resolve beside the config file.
- Custom sounds must be MP3 files of at most 16 MiB. A file that cannot be read or decoded falls back to the built-in sound.
- `HERDR_DISABLE_SOUND`, when set, turns playback off.

Volume is Sheprd's own, since Herdr's `[ui.sound]` has none. Set it in `config-gpui.local.toml` or **Settings > Notifications > Volume**, from 0 to 100 (default 50):

```toml
[sound]
volume = 30
```

It follows loudness, so 50 plays about 12 dB quieter than the file. `sheprd sound play done` (or `request`) plays a sound at that volume, and **QA > Play Sound** tests playback with the built-in Done sound.

Each client plays its own sounds, so running the TUI and Sheprd together plays both.

## Terminal bell

A program's terminal bell (BEL) never plays Herdr's sounds. The `[bell]` table in `config-gpui.local.toml` decides what it does:

```toml
[bell]
attention = true  # request attention (bounce the Dock) while the window is inactive
sound = false     # play the system alert sound
```

A burst rings at most once per 500 ms. **QA > Ring Bell in 3 Seconds** previews both, giving you time to switch to another app.

Herdr 2 terminals ring the same way while their tab is in the workspace shown; a tab in another workspace stays quiet. A mirror tab never rings: the tab driving the terminal does.

## Dock and taskbar badges

On **macOS**, the Dock icon shows how many agents are done or blocked, across all connected hosts and windows, and clears at zero. It follows the daemon's status like the sidebar, so bringing the app forward does not clear it; blocked agents stay counted until their status changes. Muted sounds, dismissed toasts, and the delivery setting do not affect it.

macOS draws the badge only when **Badges** is allowed for Sheprd in **System Settings > Notifications**. The first time the count rises above zero, Sheprd asks for notification permission including badges. If it was already allowed without badges, turn **Badges** on there.

On **Windows**, the taskbar button carries a red overlay with the same count, `9+` past nine. Screen readers hear "N agents need attention". The button also fills in while Sheprd copies dropped files to an SSH host, teleports a worktree, moves a pane to another host, or installs an update.

**Linux** shows no badge.

> **Note.** The badge and notifications work only while Sheprd runs. Quitting stops them; it is not a background service.

To preview the badge, choose **QA > Enable badge** (on Windows, in the window's menu). It shows at least `2` until you choose **QA > Disable badge preview** or quit.
