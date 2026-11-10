---
title: Workspaces and worktrees
blurb: One sidebar row per checkout, with the status that matters.
status: shipped
---

# Workspaces and worktrees

This page covers how Sheprd shows your workspaces and Git worktrees, how to create, open and delete worktrees, the scripts a repository can run for them, and the notes you can keep on each checkout.

## The sidebar

Each Herdr workspace is a row in the sidebar's Spaces list. Linked Git worktrees nest under their repository's main checkout, and a repository groups its worktrees only while one of them is open. Each row shows:

- the branch, and a dot for each agent's status (idle, working, waiting on you, done), taken from the daemon, so the terminal client and Sheprd always show the same dot
- the pull request number for the branch, colored by its checks, reviews and merge state (needs a GitHub sign-in; see [Review and Git](review.md))
- a yellow pencil when the checkout has uncommitted work
- a green `↑` for commits to push and a red `↓` for commits to pull

By default a row's change counts are the pull request's. Set `change_counts = "local"` under `[sidebar]` (**Settings > Sidebar > Change counts**) to show the checkout's uncommitted `+added/-deleted` lines instead. Local counts cover checkouts on this machine only.

With several hosts listed, workspaces are grouped under a row per host, and that row stays pinned at the top while you scroll its workspaces.

## The workspace menu

Right-click a workspace or worktree row to open its popover. Double-click a row to rename it. The popover has tiles for the common actions and rows for the rarer ones:

- **New worktree**, **Fan out**, **Rename**, **Teleport**, **Teleport back** and **Close** (**Close group** on a main checkout whose linked worktrees are open)
- **Checkpoints...**, to roll the checkout's files back (see [Review and Git](review.md#checkpoints))
- **Note...**, **Open worktree...**, **Run script** and **Run setup script**
- a **Pull request** card for the branch, when you are signed in to GitHub
- **Delete worktree checkout**, last, for linked worktrees

Closing a workspace ends its terminals but never touches checkout files or branches. Before it closes, Sheprd checks every affected local checkout for uncommitted files and for commits that are on no remote-tracking branch. If it finds either, or cannot check (for example on a remote host), you type `close` to confirm.

For moving a worktree to another machine, see [Teleport](teleport.md).

## Creating a worktree

Choose **New worktree** from a workspace's menu, or press Cmd-N (Ctrl+Shift+N on Linux and Windows) to open the New worktree dialog for the focused workspace. From a linked worktree, the new worktree starts from that worktree's branch.

- The dialog opens on the **Branch** field with a proposed name selected, so typing replaces it. Characters Git does not allow in a branch name become a dash as you type.
- **Name** is optional. Left empty, the daemon picks the workspace name. A name you type also shapes the proposed branch: "Fix login" proposes `worktree/fix-login`, until you edit the branch yourself.
- A branch that already exists is checked out as it is. One still checked out in another worktree is refused with Git's message.
- The dialog also has pull request and issue tabs. Picking a pull request fetches its head and checks it out (a fork's pull request becomes a local `pr/<number>` branch); picking an issue creates a new branch named after it. These tabs need a GitHub account signed in to Sheprd. A GitLab project fills the issues tab without one; GitLab merge requests are not listed.

Tab and Shift-Tab move between the fields; Ctrl-Tab and Ctrl-Shift-Tab step through the dialog's tabs.

If the dialog cannot open, for example because the workspace is not a Git repository, its main checkout is not open, or it has a detached `HEAD`, a short message says why.

Herdr 2 (herdr-core) keeps no worktrees, so in a workspace Sheprd keeps for Herdr 2 terminals, Sheprd runs Git itself: **New worktree** adds the checkout in the folder Herdr 1 would use (`~/.herdr/worktrees/<repository>/<branch>`) and opens it as a new workspace, and **Delete worktree checkout** removes the checkout and closes its workspace, keeping the branch. The dialog's **Existing** tab lists the repository's checkouts no workspace has open. Worktree scripts do not run there yet.

**Open worktree...** lists the repository's existing checkouts. Type to filter by branch, label or path, then press Enter or click a row to open it.

## Where a new worktree opens

The New worktree dialog's **Opens in** row chooses what the new checkout's first pane shows: **Terminal**, a plain shell, or one of the agents installed on the host, such as `claude` or `codex`. Sheprd starts the agent in that shell once the checkout exists, even if you closed the dialog while it was being created.

The agents are looked up on the host when the dialog opens. Your choice is offered first next time, in every window and after a restart; if the host lacks it, the dialog falls back to Terminal. If the agent fails to start, the window says so and the shell stays open in the new checkout.

> **Note.** The **Opens in** row is not shown on a Windows client, for WSL distributions, or when the dialog creates the checkout on another host.

## Worktree scripts

A repository can commit scripts that prepare, start and clean up its worktrees, in `.herdr/worktree.toml` at the repository root:

```toml
[scripts]
# Runs when a new worktree is created from the New worktree dialog.
setup = """
cp "$HERDR_ROOT_PATH/.env" .env
ln -s "$HERDR_ROOT_PATH/node_modules" node_modules
"""
# Runs when you choose Run script from the workspace's menu.
run = "npm run dev"
# Runs when you delete the worktree checkout, before it is removed.
archive = "docker compose down"
```

Every key is optional; an unknown key is reported as an error. The file is read from the checkout the script is for, so each branch can carry its own.

- **setup** runs after a worktree is created from the New worktree dialog. Opening an existing checkout does not run it. **Run setup script** in a linked worktree's menu runs it again.
- **run** starts from **Run script** in the menu of any Git checkout.
- **archive** runs when you confirm **Delete worktree checkout**. The checkout is removed only if the script succeeds; if it fails, its tab stays open on the output and the checkout stays.

Each script runs in a new tab named `setup`, `run` or `archive` in the checkout's workspace, with `sh -e`, so it stops at the first failing command. It runs in the checkout, with `HERDR_WORKTREE_PATH` set to the checkout and `HERDR_ROOT_PATH` to the repository's main checkout. On a remote host the script is read and run on that host, which needs `sh`. WSL distributions are not supported yet.

### Trusting a script

The file comes from the repository, so nothing in it runs until you trust it. The first time a script would run, a dialog shows every script in the file in full.

- **Trust and run** remembers that exact file for that repository on that host. Any change to the file, such as a pulled commit, asks again.
- **Don't run** runs nothing. For an archive script, **Remove without running** removes the checkout anyway.

Trust is kept in `trusted-worktree-scripts.json` in Sheprd's state directory; delete that file to forget every grant.

Agents can list and run these scripts with `sheprd scripts`, `sheprd scripts setup` and `sheprd scripts run`. A script the app has not seen still asks first. See [CLI](cli.md).

## Worktree notes

Right-click a Git checkout and choose **Note...** to keep a reminder on it, such as what it is waiting on. A row with a note shows a note icon, and the note's first line under the row; click that line to edit it. Saving an empty note removes it.

- **Worktree Notes** in the command palette lists every open checkout with a note on any connected host, most recently edited first. The palette also finds a workspace by its note's text.
- **Edit Worktree Note** opens the focused checkout's note.
- Neither has a default shortcut; bind `worktree_notes` and `edit_worktree_note` under `[keybindings]` (see [Keyboard](keyboard.md)).

A note is one line of at most 500 characters. Notes stay in Sheprd, in `worktree-notes.json` in its state directory: they never reach the daemon or the repository. A note belongs to a host, repository and branch, so it survives a daemon restart and follows its branch. It does not travel with a teleported worktree.

From a pane, `sheprd notes set TEXT`, `sheprd notes get` and `sheprd notes clear` work on the note of your own checkout.

## Telling worktrees apart

A linked worktree looks different from its repository's main checkout, so you can see which copy you are about to type into.

- In the sidebar, linked worktrees hang from their main checkout on thin guide lines. A linked worktree whose main checkout is not open shows a branch glyph in the theme's blue instead.
- Optionally, a 2-pixel rule in the same blue runs along the tab strip's edge in a linked worktree's workspace. It is off by default.

The `[worktrees]` table changes the color and where the marks show:

```toml
[worktrees]
# The glyph's and the rule's hue, as #rgb or #rrggbb; it only gets lighter or
# darker as far as it must to stand out from the chrome. Unset, the theme's
# blue.
tint = "#7aa2f7"
# Where the marks show; [] turns them off. Without "sidebar", the flat
# layouts keep their tree and the others draw no guides, as before. The
# default is ["sidebar"]; add "terminal" for the rule along the tab strip.
apply_to = ["sidebar", "terminal"]
```

`sheprd config set worktrees.apply_to '[]'` sets it from a script.

## Deleting a worktree

**Delete worktree checkout** in a linked worktree's menu removes the checkout after one confirmation. If the checkout has uncommitted or untracked files, Herdr refuses and the dialog offers **Force remove**, which asks again. Unpushed commits are not checked. If the repository has an archive script, it runs first (see above).

To clean up automatically, set `remove_merged_worktrees = true` (**Settings > General > Remove worktrees of merged PRs**). Once a linked worktree's GitHub pull request is merged, Sheprd removes its checkout and closes its workspace, without asking. It is off by default and acts only when:

- the worktree is on this machine's daemon
- the checkout has no uncommitted or untracked files and sits exactly at the pull request's merged head commit
- no agent in it is working or waiting on a prompt

The focused workspace opens the delete dialog instead, a repository with an archive script is left for that dialog, and the branch is never deleted.
