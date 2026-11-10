---
title: Install
blurb: Homebrew, Linux, Windows, Nix and the iOS beta.
status: shipped
platforms: macOS, Linux, Windows
---

# Install

Install the latest stable Sheprd release on macOS, Linux or Windows, or build it with Nix. This page also covers the iPhone and iPad beta and what you need from Herdr.

## At a glance

| Platform | Command |
| --- | --- |
| macOS | `brew install sheprd-labs/tap/sheprd` |
| Linux (APT, DNF, zypper or pacman) | `curl -fsSL https://sheprd.org/install.sh \| sh` |
| Windows (PowerShell) | `irm https://sheprd.org/install.ps1 \| iex` |
| Nix | `nix run github:sheprd-labs/sheprd` |
| iPhone and iPad (beta) | Join on TestFlight: [sheprd.org/testflight](https://sheprd.org/testflight/) |

Every release also publishes manual downloads on [GitHub Releases](https://github.com/sheprd-labs/sheprd/releases): a macOS `.dmg`, Linux `.deb`, `.rpm`, Arch packages and tarballs, and Windows `.zip` files and `-setup.exe` installers. Each comes with a SHA-256 checksum.

## Do you need Herdr?

No. Release builds include a pinned herdr-core for local terminals. If Herdr 1 is not installed, Sheprd opens a herdr-core shell and no separate daemon installation is required. It joins a herdr-core that is already running, otherwise it starts your own `herdr-core` or the bundled one.

If you already use [Herdr 1](https://herdr.dev/), Sheprd connects to its sessions and may start your installed `herdr server` when the session is not running. It does not upgrade or migrate your daemon, and it does not move your Herdr 1 sessions to Herdr 2. Uninstalling or closing Sheprd leaves daemon sessions and saved configuration in place.

SSH hosts and WSL distributions are different: Herdr must be installed there, and Sheprd never installs or upgrades it for you. See [SSH hosts](ssh-hosts.md).

## macOS

You need [Homebrew](https://brew.sh/) and macOS 14.2 Sonoma or newer, on Apple Silicon or Intel. The cask installs the signed, notarized universal app.

```sh
brew install sheprd-labs/tap/sheprd
open /Applications/Sheprd.app
```

You do not need `--cask`. Use the full name: it taps `sheprd-labs/tap` and trusts the cask for you, which Homebrew requires for a non-official tap. Plain `brew install sheprd` only works after `brew trust --cask sheprd-labs/tap/sheprd`.

A Homebrew install updates through Homebrew. The in-app updater detects this and runs `brew upgrade --cask` for you. See [Updates](updates.md).

> **Older installs.** If you installed from the older `penso/tap`, you keep receiving upgrades there as `herdr-gpui`, installed at `Herdr.app`. Every release is published to both taps.

## Linux

Releases include x86_64 and ARM64 builds as a `.deb`, an `.rpm`, an Arch Linux package and a plain tarball. They need glibc 2.39 or newer: Ubuntu 24.04, Debian 13, Fedora 40, current Arch, openSUSE Tumbleweed or Leap 16, or later.

```sh
curl -fsSL https://sheprd.org/install.sh | sh
```

The script, which you can read first at [sheprd.org/install.sh](https://sheprd.org/install.sh):

1. Checks that you are on Linux and picks `x86_64` or `aarch64`.
2. Picks the package format from your package manager: APT (`.deb`), DNF or zypper (`.rpm`) or pacman (`.pkg.tar.zst`). Without one of them it stops and points you to the tarball or Nix.
3. Finds the latest stable release (betas are never picked), downloads the package and its `.sha256` file, and stops on a checksum mismatch.
4. Installs it with `sudo apt-get install`, `sudo dnf install`, `sudo zypper install` or `sudo pacman -U` (no `sudo` when run as root).

It needs `curl` and `sha256sum`. Once installed, start Sheprd from your applications menu or run `sheprd`.

To install by hand, download the file for your architecture from Releases, then:

```sh
sudo apt install ./Sheprd-VERSION-x86_64-unknown-linux-gnu.deb      # Debian, Ubuntu
sudo dnf install ./Sheprd-VERSION-x86_64-unknown-linux-gnu.rpm      # Fedora
sudo pacman -U Sheprd-VERSION-x86_64-unknown-linux-gnu.pkg.tar.zst  # Arch Linux
# openSUSE: the release RPM is unsigned, which zypper stops on without the flag
sudo zypper install --allow-unsigned-rpm ./Sheprd-VERSION-x86_64-unknown-linux-gnu.rpm
```

The package is still named `herdr-gpui` internally. It provides the `sheprd` command and a `/usr/bin/herdr-gpui` compatibility executable. Your package manager pulls in the runtime libraries, including the Vulkan loader. App Updates installs a newer `.rpm` through DNF or, on openSUSE, zypper. See [Updates](updates.md).

### Vulkan driver

Sheprd renders with Vulkan, so a Vulkan driver for your GPU must be installed. On Arch Linux, install `vulkan-radeon` for AMD or `vulkan-intel` for Intel. On a laptop with two GPUs, install the driver for the GPU your display uses, not only the discrete one. Otherwise Sheprd renders on the discrete GPU, which can sleep while the window is hidden, leaving the window blank for a second or more when you return to it.

Terminal clipboard paste uses `wl-paste` (the `wl-clipboard` package) on Wayland or `xclip` on X11. The packages and the Nix build include these.

## Windows

In PowerShell, on x86_64 or ARM64:

```powershell
irm https://sheprd.org/install.ps1 | iex
```

The script, readable at [sheprd.org/install.ps1](https://sheprd.org/install.ps1):

- Downloads the zip for your architecture from the latest stable release and checks its SHA-256.
- Installs `sheprd.exe`, `sheprd-launcher.exe`, the companion helpers and herdr-core for your user only, under `%LOCALAPPDATA%\Programs\Herdr UI`. The old `herdr-gpui.exe` and `herdr-gpui-launcher.exe` names stay as aliases.
- Adds that folder to your user `PATH` and creates a **Sheprd** Start menu shortcut that opens the launcher, so no console window appears.
- Adds **Open in Sheprd** to folders in File Explorer (on Windows 11, under **Show more options**) and registers `sheprd://` and `herdr-gpui://` links.

It never starts or stops terminal daemons, changes services or settings, or upgrades a personal Herdr. Close Sheprd before rerunning it to update: Windows will not replace a running file, and the script refuses rather than stopping anything. If the automation daemon is running, the script asks it to stop once its runs finish and starts the new copy afterwards.

To remove everything the script added, keeping your settings and daemon sessions:

```powershell
& ([scriptblock]::Create((irm https://sheprd.org/install.ps1))) -Uninstall
```

### Regular installer

Each [release](https://github.com/sheprd-labs/sheprd/releases/latest) also has `Sheprd-VERSION-x86_64-pc-windows-msvc-setup.exe` and `Sheprd-VERSION-aarch64-pc-windows-msvc-setup.exe`. They install the same files to the same place for the current user, without administrator rights, and add an uninstaller under **Settings > Apps**.

`sheprd.exe` is a console program because it is also the [command line](cli.md). Open the app with `sheprd-launcher.exe`, which is what the Start menu shortcut does. See [Windows](windows.md) for what works on the platform.

## Nix

On NixOS, or anywhere with Nix, build from source with the repository's flake on x86_64 or ARM64 Linux:

```sh
nix run github:sheprd-labs/sheprd
# or add it to a configuration: inputs.sheprd.url = "github:sheprd-labs/sheprd";
# then environment.systemPackages = [ inputs.sheprd.packages.${system}.default ];
```

A Nix install never updates itself. Update it through Nix.

## From source

Install Rust with rustup and, on macOS, the Xcode command-line tools. Then:

```sh
git clone https://github.com/sheprd-labs/sheprd.git
cd sheprd
just install
```

This installs the optimized `sheprd` executable into Cargo's bin directory. On macOS it installs the standalone executable only; use Homebrew for the full app bundle. Linux and Windows build dependencies are listed in the [README](https://github.com/sheprd-labs/sheprd#from-source). Sheprd is not published on crates.io, so `cargo install sheprd` does not work. Source builds need their own herdr-core binary for Herdr 2 terminals.

## iPhone and iPad

The native iPhone and iPad app is a TestFlight beta and needs iOS 26. Nothing runs on the phone itself: the app pairs with Sheprd on your computer.

1. Install Apple's free TestFlight app from the App Store.
2. Open [sheprd.org/testflight](https://sheprd.org/testflight/) on the device, tap **Join the TestFlight beta**, then **Accept** and **Install** in TestFlight. New builds arrive there as updates.
3. In Sheprd on your computer, turn on remote control in Settings and open pairing. Scan the QR code with the app, or keep the phone nearby to pair over Bluetooth.

Beta builds can change or break between updates, and the invite link can change between betas; the TestFlight page always has the current one. See [Remote control](remote-control.md).
