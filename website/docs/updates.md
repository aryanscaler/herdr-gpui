---
title: Updates
blurb: Stable and beta channels, and how each install updates.
status: shipped
platforms: macOS, Linux, Windows
---

# Updates

Sheprd updates itself from inside the app, through whatever installed it: Homebrew, your Linux package manager, or the Windows installer. This page covers the update channels, what the updater checks, and how each kind of install is updated.

## How the updater works

Release builds check for a new version when they start, every hour while idle, and when you ask. To check now, choose **Sheprd > Check for Updates...**, or open **app updates** from the sidebar menu. When an update is available, the version label in the status bar changes; the panel does not take focus on its own.

Installing is always your decision. **Download** and **Install Update** are separate approvals. Closing the panel does not cancel a download; use **Cancel** for that. The panel shows download progress, or an animated bar for steps without a known percentage.

Updating the app never stops or upgrades the terminal daemon. Your terminals and agents keep running through the restart.

From a terminal, `sheprd update status` and `sheprd update check` report on updates. They never download or install anything.

## Update channels

There are two channels:

- **Stable**, the default.
- **Beta.** Betas are published as GitHub prereleases before being promoted to stable. Turn on **Settings > General > Install beta releases** to receive them.

The same setting in `config-gpui.local.toml`:

```toml
[updates]
channel = "beta"
```

Turning beta off never downgrades you: you stay on the beta until a newer stable release arrives. A beta that is promoted to stable is the exact same build, so stable users run the bytes beta users already ran.

Betas never reach the Homebrew tap. On a Homebrew install with the beta setting on, the app updates itself in place, and Homebrew takes it back at its next upgrade.

## Signed updates

Every release publishes a small manifest signed with Sheprd's Ed25519 key, whose public half is built into the app. The updater only offers and installs a release whose manifest checks out, and it verifies each download's size and SHA-256 against it. On top of that:

- **macOS.** The new app must be signed by the same Apple Developer ID team, with the same bundle identifier and the expected version. Releases are also notarized by Apple.
- **Linux.** The package's signature, size and SHA-256 are checked again right before it is installed.
- **Windows.** The installer must match the signed manifest and carry a valid Authenticode signature from Sheprd's publisher, or it never runs.

Local and source builds, which have no release version or embedded key, never check for updates.

## By install method

### Homebrew on macOS

The updater notices that Homebrew owns the app and runs `brew upgrade --cask` on its cask, so Homebrew's records stay correct. If the upgrade does not reach the offered version, it runs `brew update` and retries once. The tap is published shortly after each release, so it can briefly lag. A Homebrew upgrade is never cancelled once it starts. Choose **Restart** when it finishes.

### Standalone macOS app

An app installed from the `.dmg` is replaced in place. It must be owned by you, in a folder you can write to without elevation; the usual `/Applications` is fine. The previous copy is kept in an adjacent `.herdr-update-*` folder in case you need it, and you remove it yourself. An older `Herdr.app` is installed as `Sheprd.app` on update, leaving a hidden `Herdr.app` link behind so existing paths keep working.

### Linux packages

For a `.deb`, `.rpm` or Arch package, the updater finds which package manager owns the app, downloads the matching signed package, and installs it through APT, DNF, zypper or pacman. You authorize it in a PolicyKit prompt; the app never asks for your password itself. This needs `pkexec` and your desktop's PolicyKit authentication agent. Once started, a package install cannot be cancelled. Choose **Restart** when it finishes.

Builds that report "Package-managed installations cannot self-update" are older; upgrade the package by hand once to get in-app updates.

A standalone Linux executable updates only when it is an ordinary file you own under your home folder, such as `~/.local/bin/sheprd`. AppImage, Snap, Flatpak and Nix installs are left to whatever manages them, and the Nix flake never updates itself.

### Windows

Copies installed by the installer, winget or `install.ps1` in `%LOCALAPPDATA%\Programs\Herdr UI` update in the app. The updater downloads the release's per-user installer, checks it, runs it silently, and reopens Sheprd. A failed install leaves the previous version in place. A copy run from anywhere else does not update itself; download new releases manually.

## Turning updates off

There is no setting to stop update checks. Nothing is downloaded or installed until you approve it in the update panel, so you can ignore an offer for as long as you like. A Homebrew install can also be upgraded with Homebrew directly, and a Nix install only updates through Nix.

## If something goes wrong

The update panel reports failures, with manual-install guidance when an install cannot be updated in place. Manual downloads for every release are on [GitHub Releases](https://github.com/sheprd-labs/sheprd/releases). See [Troubleshooting](troubleshooting.md) for logs.
