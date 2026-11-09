---
title: Terminals
blurb: Native panes, splits, selection, links and file drops.
status: shipped
---

# Terminals

This page covers terminal panes in Sheprd: the two kinds of terminal, splitting and moving panes, selecting and copying text, opening links, pasting images, and dropping files, including onto SSH hosts.

## Herdr 1 and Herdr 2 terminals

A Herdr daemon owns every terminal, so closing Sheprd leaves your terminals and agents running. Sheprd works with two daemons side by side, in the same workspaces:

- **Herdr 1** (`herdr server`) runs terminal tabs and sends Sheprd their cells to draw.
- **Herdr 2** (herdr-core) streams each terminal's raw output, which Sheprd emulates and draws with the same painter. Every release bundles a pinned herdr-core.

Nothing moves from one to the other on its own: Herdr 1 tabs stay Herdr 1 tabs. [Teleport](teleport.md) moves a worktree's tabs into another host's Herdr 2 when new terminals use Herdr 2.

To open a Herdr 2 terminal, run **New Herdr 2 Terminal** in the command palette, or `sheprd run new_core_terminal`. To make every new tab a Herdr 2 terminal, turn on **Settings > Tabs > New terminals use Herdr 2**:

```toml
herdr_2_terminals = true
```

On this computer, Sheprd joins a herdr-core that is already running, or starts your own `herdr-core` or the bundled one when a Herdr 2 terminal is first needed. `herdr-core ls` lists its terminals.

A Herdr 2 terminal in a workspace on an SSH host, cloud machine, or WSL distribution runs in that host's herdr-core. Sheprd opens the connection itself, the same way it reaches the host's Herdr, and runs `herdr-core connect` there, which starts the host's core when none is running. Sheprd looks for `herdr-core` on the host's `PATH`, then in `~/.local/bin` and the usual Homebrew and Nix locations. A host without herdr-core keeps its new tabs in its Herdr. To add one, choose **Install Herdr 2** in the host's menu (right-click its header in the sidebar): Sheprd downloads the herdr-core it bundles there, checks it against its pinned checksum, and puts it in `~/.local/bin`, leaving a host's own herdr-core as it is. If Herdr 1 is not installed, the window opens a herdr-core shell instead of asking you to install Herdr.

Herdr's shared `[ui]` settings apply to Herdr 2 panes too: `copy_on_select`, `mouse_scroll_lines`, and `right_click_passthrough_modifier` (a Herdr 2 pane has no pane menu yet, so its other right-clicks also reach a program that reads the mouse).

A Herdr 2 pane keeps 10,000 lines of scrollback. herdr-core itself keeps each terminal's recent output, a few thousand lines, through Sheprd and herdr-core restarts, and a pane shows it again when it opens, so scrolling back after a restart works without a switch. **Restore scrollback after Herdr restarts** and Herdr's `[advanced] scrollback_limit_bytes` apply to Herdr 1 only.

**Mirror Herdr 2 Pane** opens a watch-only view of the focused pane in a new tab. In the sidebar, a Herdr 2 list under the agents shows each pane of the workspace's Herdr 2 tabs with its status.

## Splits

**Split Right** (Cmd-D on macOS, Ctrl+Shift+D on Linux and Windows) and **Split Down** (Cmd-Shift-D, or Ctrl+Alt+Shift+D) split the focused pane. Focus Left, Right, Up and Down move between panes, **Toggle Pane Zoom** grows one pane over the tab, and **Close Pane** ends that pane's terminal. See [Keyboard](keyboard.md) for the keys.

Right-click any pane, even one that is not focused, for Rename, Split Right, Split Down, Toggle Zoom and Close, and, on another pane of the focused tab, **Swap with Focused Pane**.

In a Herdr 2 tab, drag the line between two panes to resize them; the layout is saved with the tab, and a pane whose program exits closes. A split takes the style of the tab layout: Classic gives each pane a header row, Curved draws rounded cards, Pills floats a label in each pane's corner, and Underline ends each pane in a status line.

## Moving panes

Right-click a pane, or an agent's row in the sidebar, for:

- **Move to New Tab**, in the pane's own workspace
- **Move to New Workspace**
- **Move to…**, which lists the other tabs of every workspace and a new tab in each other workspace

The command palette's **Move Pane…** (`move_pane`, unbound by default) opens the same list for the focused pane. Moving into an existing tab splits that tab's focused pane and puts the moved pane on its right. The terminal keeps running throughout.

Herdr refuses to move a pane out of or into a zoomed tab, and the menu shows its reason.

> **Note.** Moving panes is available on macOS and Linux clients, for the local daemon, a named session, or an SSH device. **Move to…** also lists other devices; see [Teleport](teleport.md) for moving work between machines.

## Selection and copy

Drag across the terminal to select. Releasing the button copies the selection and shows a `copied to clipboard` flash. Double-click selects a word and triple-click a line in Herdr 2 panes. In a Herdr 2 pane, hold the drag past the top or bottom edge to scroll and select into the scrollback; Cmd-C copies all of it.

- The highlight stays until the next click or keystroke, and Cmd-C or **Edit > Copy** copies it again. Ctrl-C still goes to the pane. Set `keep_selection_after_copy = false` in `config-gpui.local.toml` to clear it on release.
- With Herdr's `[ui] copy_on_select = false`, releasing copies nothing; press Cmd-C, Ctrl-C, or **Edit > Copy**.
- A selection stays inside the pane it started in. Dragging past a pane's top or bottom edge scrolls it.
- Concealed text copies as blanks. Very large copies are refused with a message in the status bar.

Programs that read the mouse, such as many agent TUIs, receive your clicks and drags. **Hold Shift** to select text there instead. The first time such a program takes a drag, Sheprd shows a short reminder.

Programs that copy with OSC 52 put their text on your clipboard too, with the same flash, in Herdr 1 and Herdr 2 panes. Only bounded UTF-8 text (up to 2 MiB) is accepted, and programs can never read the clipboard this way.

**Paste** is Cmd-V on macOS, Ctrl-Shift-V on Linux, and Ctrl-V (while the clipboard holds text) or Ctrl-Shift-V on Windows.

### Right-click

Right-click opens the pane menu, unless the pane routes right-clicks to its program (`herdr pane input --right-click pane`, or **Send Right-Clicks to Pane** in the pane menu). In a routed pane, right-click with Shift, Control, Option or Command still opens the menu, where **Open This Menu on Right-Click** switches it back.

### Find and copy mode

- **Find** (Cmd-F, or Ctrl+Shift+F) searches the focused pane's whole scrollback. Enter and Shift-Enter, or Cmd-G and Cmd-Shift-G, step through matches. A query with an uppercase letter is case-sensitive.
- **Copy Mode** (Cmd-Shift-C, or Ctrl+Alt+Shift+C) moves a block cursor through the scrollback from the keyboard: `h` `j` `k` `l` and arrows step, `w` `b` `e` move by word, `/` and `?` search, `v` or Space marks by cell and `V` by line, `y` or Enter copies, and `q` or Escape leaves.
- **Clear Pane** (Cmd-K, or Ctrl+Shift+K) clears the screen and scrollback without sending input to the program.
- **Edit Scrollback** opens the pane's scrollback in `$VISUAL` or `$EDITOR` (`vi` by default, Notepad on Windows). The file is removed when the editor quits.
- In a Herdr 2 pane, Sheprd keeps the scrollback itself, so Find, Copy Mode, Clear Pane, and Edit Scrollback work whatever Herdr 1 is installed. Clear Pane clears only that view: a mirror of the pane, or the phone, keeps what it shows. Edit Scrollback opens the editor in a new pane beside it, which closes when the editor quits.

### Annotating terminal text

Hold **Shift** while selecting text in a pane. When you release, an **Annotate** pill appears beside the selection. Click it and write a note in the **Message to agent…** field:

- **Enter** sends the note.
- **Shift+Enter** queues it so you can select more text (up to 20 notes).
- **Escape** dismisses the field.

For an existing selection, run **Annotate Selection for the Agent** (Cmd-Shift-A on macOS, Ctrl+Alt+Shift+A on Linux and Windows). The whole selection must be on screen. Notes go to the agent in that pane, else to the first agent in its workspace, and are delivered as page notes are (see [Browser tabs](browser.md#sending-notes)). With no agent, they are copied to the clipboard.

In a Herdr 2 pane, select text (it may reach into the scrollback) and run **Annotate Selection for the Agent**. Herdr 2 agents do not take notes yet, so the note is copied to the clipboard.

## Terminal links

Cmd-click (Ctrl-click on Linux and Windows) a link in a pane to open it. A click without the modifier only selects or goes to the program.

- **A dashed underline** marks text that may be a link but is not verified yet.
- **A solid underline** marks a verified link.
- **A hand cursor** marks text a click acts on.

Holding the modifier over text checks it. Web addresses (`http://`, `https://`), `mailto:` and `vscode:` are verified when well formed. A file path such as `src/main.rs:12:5`, `./build/out`, `~/notes.md` or a `file://` address is verified when it exists; relative paths resolve against the pane's working directory. An explicit terminal hyperlink with another scheme shows its address and asks before opening.

A file opens in your terminal editor at the printed line, in a new pane split to the right (see [Editors](editors.md)). Folders, images, videos and PDFs open in the system's default application. Holding Alt (Option on macOS) as well opens it the other way. Two settings choose the defaults:

```toml
open_links_in = "system"   # or "browser-tab", macOS and Windows only
open_files_in = "editor"   # or "system"
```

Paths are not links in panes on SSH hosts, whose files live on that host. On Windows, files always open in the system's application.

Herdr 2 panes read links from their own screen, the same way, and a web address the terminal wrapped onto the next row opens as one link. A file opens in the editor in a new Herdr 2 pane split to the right, and quitting the editor closes it. Plugin `[[link_handlers]]` belong to Herdr 1 and see no clicks in Herdr 2 panes.

## Images

Paste an image into an agent with Cmd-V. Sheprd sends it through Herdr's image bridge: the daemon writes a temporary file and pastes its path into the pane, where an agent such as OpenCode can read it as an image. Sheprd never presses Enter for you.

- **Local panes:** Cmd-V pastes a clipboard image (Ctrl-Shift-V or Shift-Insert on Windows). Local Ctrl-V goes to the terminal unchanged, so agents can read the clipboard themselves.
- **SSH panes:** drop one PNG, JPEG, GIF, WebP or BMP image on a pane, or paste one with Cmd-V or Ctrl-V.
- Images up to 16 MiB pass unchanged. Larger static images are recompressed or downscaled to fit; oversized animated images are refused.
- The temporary files belong to the connection; Herdr removes them when the client disconnects.

On Linux, pasting needs `wl-paste` (from `wl-clipboard`) on Wayland or `xclip` on X11.

Programs that show images with the kitty graphics protocol, such as `kitten icat`, show them in Herdr 1 and Herdr 2 panes alike. In a Herdr 2 pane only images sent in the output itself are shown, never a file or shared memory a program names, and an image scrolls and clears with the text around it. A Herdr 2 pane opened after an image was drawn, such as after restarting Sheprd, shows the text but not the image.

## File drops

Drop files from your file manager onto a pane or popup to paste their paths there, even if another pane is focused. Paths are pasted with shell characters escaped (`/tmp/Shot\ 1.png`), separated by spaces, and Enter is never pressed. Files copied in the file manager and pasted with Cmd-V work the same way. A drop is limited to 256 paths.

Herdr 2 panes take dropped and copied files the same way. A live macOS screenshot thumbnail, dropped before it saves, goes through the image bridge as a pasted image does on a Herdr 1 pane; a Herdr 2 pane, which has no image bridge, gets its path. A WSL distribution receives its own paths for the files, with nothing copied.

## SSH file copies

Drop regular files onto an SSH pane to copy them to that host. A `Copying...` card shows the file name or count, progress, and a **Cancel** button. Once every file has arrived, their remote paths are pasted into the pane. Nothing is pasted if the copy fails.

- Each file goes into a new private `herdr-upload.*` directory under the host's `${TMPDIR:-/tmp}`. Existing files are never overwritten.
- Directories and special files are refused; symlinks to regular files are followed. A single image uses the image bridge instead.
- Copies use a separate SSH connection, so typing in the terminal stays responsive. Wait for the copy to finish before running a command that needs the file.
- Switching hosts, reconnecting or closing the window cancels the copy. A copy with no progress for 30 seconds stops.
- Copied files stay on the host until you remove them or the host cleans its temporary directory.

> **Windows.** Files and images dropped or pasted on an SSH host's pane are not uploaded from a Windows client yet.
