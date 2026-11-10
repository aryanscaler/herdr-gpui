---
title: Work across machines
blurb: Move panes and worktrees, pick an idle host, fan out.
status: shipped
platforms: macOS, Linux
---

# Work across machines

When you have more than one machine connected, Sheprd can move work between them and choose where new work goes. This page covers moving one pane to another device, Teleport for a whole worktree, Smart Dispatch, and fan-out across hosts.

> **macOS and Linux clients.** Pane moves to other devices, Teleport, Smart Dispatch, and fan-out run scripts on your computer, so they are not available from a Windows client. They reach your local Herdr session, saved [SSH hosts](ssh-hosts.md), and some [cloud machines](cloud.md); not a custom socket, a WSL distribution, or a development (`--dev`) session.

## Moving a pane to another device

Right-click a pane, or an agent's row in the sidebar, and choose **Move to…**. After the other tabs and workspaces, the list shows each other connected device that has the pane's repository open, with two choices:

- **DEVICE › New Worktree** copies the pane's work into a new worktree there.
- **DEVICE › SPACE** copies it into that device's existing space of the repository.

The two devices must share a Git remote; a repository matching only by name is refused. The review lists the branch, the unpushed commits and changed files, and what the pane will run there. Press Enter to start the move.

The checkout's commits and its staged, unstaged, and untracked changes go with the pane. A space takes them only if nothing there would be overwritten: its checkout must have no uncommitted changes and be at the pane's commit, or behind it on the same branch. The pane then runs in a new tab there. A supported agent's session moves and resumes; another program starts again with the same arguments, in the same subdirectory. The work is copied, so the checkout here keeps its changes and the other panes keep running. The pane here closes only once it runs on the other device, and the window follows it.

A working agent is still writing its session, so it moves only once it is idle or waiting.

### Agent session only

Outside a Git repository, **DEVICE › Agent Session Only** moves a supported agent's session into a new workspace on that device, with no files copied. Its folder maps relative to home: `~/notes` here becomes `~/notes` there, and that folder must already exist. The same agent must be installed there.

## Teleport

Right-click a linked worktree and choose **Teleport...** to move it to another connected host. It takes:

- its branch and commits,
- staged, unstaged, and untracked changes, binary files included,
- tabs and splits, with their custom labels,
- the programs running in them, started again with their arguments,
- agent sessions, resumed with their full history.

Herdr 2 tabs in the worktree's workspace move too, after its Herdr tabs and with their splits. They run in the destination's Herdr 2 when it has `herdr-core`, and in its Herdr otherwise. With **Settings > Tabs > New terminals use Herdr 2** turned on, the Herdr tabs move into the destination's Herdr 2 as well, without their labels, and the workspace there keeps one Herdr shell tab. The review marks each tab that will run in Herdr 2. A tab of more than 16 panes stays in Herdr. Herdr 2 reports no agent session, so Sheprd reads Claude Code's own record of the running process (`~/.claude/sessions/<pid>.json`, checked against the process's start time) and resumes that session on the destination. Other agents in Herdr 2 write a handoff note and start again with it, rather than resuming.

A Herdr 2 workspace on this computer, one Sheprd keeps with no Herdr workspace behind it, teleports the same way. Work coming to this computer lands in such a workspace when new workspaces here use Herdr 2 (that setting is on, or Herdr is not installed): Sheprd adds the worktree with Git under `~/.herdr/worktrees/<repository>/<branch>`, where Herdr would, and opens every tab in Herdr 2. A tab of more than 16 panes cannot open there, and the review says so. Herdr 2 workspaces on SSH hosts and cloud devices are not offered yet.

Ignored files such as `.env` and build output stay behind, and so do submodule contents, environment variables, shell history, background jobs, and unsaved editor buffers.

Teleport works on the local session, saved SSH hosts, and Coder, Daytona, and Railway cloud devices, in either direction. It does not create a cloud machine or install Herdr.

### Steps

1. **Choose a host.** Every other enabled host is listed, connected or not. Sheprd then finds where the worktree goes there: a checkout it once left, else the repository if it is open there, matched by Git remote. Otherwise it opens a checkout at the same place under the home directory (`~/code/app`), clones from `origin`, or, when that host cannot reach `origin`, copies the repository from this machine.
2. **Review.** The dialog lists the branch, unpushed commits, changed and untracked files, and what each pane becomes. The destination's branch must be absent or behind this one, and not checked out there.
3. **Teleport.** Closing the dialog does not stop a move in progress; its result arrives as a flash, and success switches to the new workspace on the destination.

From the command line, `sheprd teleport --to HOST` opens the dialog for your linked worktree, reviewing that host. You confirm in the app.

### Agent sessions

| Agent | Resumed with |
| --- | --- |
| Claude Code | `claude --resume <id>` |
| Codex | `codex resume <id>` |
| opencode | `opencode --session <id>` |
| pi, omp | `pi --session <file>`, `omp --resume=<file>` |
| GitHub Copilot CLI | `copilot --resume=<id>` |
| Letta Code | `letta --conversation <id>` (conversations stay on the Letta server, which the destination must be signed in to) |

Model and permission flags from the original command line are kept, except for Letta; initial prompts are dropped. An agent the destination lacks, or one with no reported session, is first asked to write a handoff note to `.herdr/teleport/handoff-N.md`. The same agent, or else the first installed of Claude Code, Codex, opencode, and pi, then starts there with that note.

### GitHub access on the destination

When `origin` is on GitHub and the destination cannot reach it by itself, the review says so and the move lends it this machine's `gh auth token`. The token is stored in the repository's Git directory, `.git/herdr/github-token` (mode 600), never in the working tree or a commit. If `gh` is not signed in here, the review warns that pull and push will not work there.

### After the move, and back

Once the destination is ready, the source workspace's programs stop: its tabs are replaced by one idle `teleported` shell tab, its Herdr 2 terminals end and their tabs close, and the checkout stays. The row gets a teleport icon, and its menu offers **Go to teleported copy** and **Clear teleported mark**. The copy on the destination offers **Teleport back**. Before work returns, the original checkout's current state is saved to `refs/herdr-teleport/backup/...`.

## Smart Dispatch

When more than one host is connected, the **New worktree** dialog and the **New workspace** dialog for a Git checkout offer a host for the new checkout. The three hosts with the most room are tiles, best first; every other host opens from the **Other** field. The host the window shows stays chosen until you pick another, so Enter still creates where it always did.

Hosts rank by spare cores: cores less the 5 minute load average. Each agent working there, in a Herdr pane or in a Herdr 2 terminal on a host whose Herdr 2 tabs the window has open, takes half a core off, a host that would have to clone the repository first takes one, memory above 90% halves its room, and recent checkouts of the same repository on a host add a little. The tiles stop moving after a few seconds so they do not shift under the pointer.

Choosing another host creates the checkout there through the same steps as Teleport: the repository is found, opened, or cloned, and the base commit is sent only when that host lacks it. The window switches to the new workspace when it is ready, and the repository's setup script asks for trust as a local creation does.

This computer takes new work while its new workspaces use Herdr 2 (with **Settings > Tabs > New terminals use Herdr 2**, or with no Herdr 1 installed), even when no Herdr 1 runs here. Git creates the checkout, in the folder Herdr 1 would use, and the window opens it as a Herdr 2 workspace. A worktree left unnamed gets a generated branch name. A Herdr 2 workspace's **New worktree** dialog offers other hosts too, and sends the checkout to their Herdr. Another host counts only when Herdr 1 runs there: Smart Dispatch does not yet create work in another host's Herdr 2. A checkout it sends to this computer's Herdr 2 runs the repository's setup script in a new tab, as a local creation does.

### New machines from the dialog

Once you have a container, Daytona sandbox, sprite, or OpenSandbox device, the dialogs also offer a **New container**, **New sprite**, or **New sandbox** tile, even with one host connected. A container or Daytona sandbox starts from the repository's [sandbox image](cloud.md#sandbox-images) when it has one. The new machine is saved as a device, gets Herdr, and runs the [cloud setup script](cloud.md#setup-scripts-for-new-machines).

## Fan-out

Fan-out runs one prompt across several agents, each in its own worktree, so you can compare the results, keep one, and remove the rest. Open it with `sheprd fan-out PROMPT [--agent KIND[:N]]...`, which prefills the dialog; you start it there.

**Spread agents across hosts** sends each lane to the host with the most room left. Each lane's host chip lists every host, best first, to move that lane. A lane whose agent is not installed on its host fails with a reason. Open, Keep, and the comparison work across hosts.

Fan-outs survive a quit or crash. The next launch shows an unfinished fan-out again as a comparison, and each lane says where it stopped. Nothing runs by itself: **Start agent** or **Send prompt** finishes a lane only when clicked. **Keep** removes the other worktrees after a confirmation; **Forget** lets the fan-out go and leaves every worktree as it is. Up to four fan-outs are kept.

New containers are not offered to fan-out lanes yet, because a new container has no signed-in agent to start.

A workspace Sheprd keeps for Herdr 2 terminals offers **Fan out** too, when its folder is a Git checkout. Git creates each lane's worktree, in the folder Herdr 1 would use, and each agent starts in a Herdr 2 shell there and gets the prompt once it is ready; each lane opens as a Herdr 2 workspace behind the one you are in. Compare, **Keep** and resuming after a quit work as on Herdr 1, and keeping a lane closes the other lanes' workspaces. These lanes stay on this computer: spreading across hosts needs a Herdr 1 workspace of the repository, and another host's Herdr 2 takes no lanes yet.
