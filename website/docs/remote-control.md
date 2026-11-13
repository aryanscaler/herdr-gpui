---
title: Remote control
blurb: Answer your agents from your phone.
status: optin
platforms: macOS, Linux, Windows
---

# Remote control

Remote control lets a phone, or any browser, follow the chats running on your computer, answer agents' questions, and get a notification when an agent needs you or finishes. This page covers turning it on, pairing a phone, the installable web app, notifications, reaching your other machines, the iPhone and iPad app, and the `sheprd remote` commands.

## How it works

Turning on remote control installs a small daemon, `sheprd-remote-control`, as a user service: a LaunchAgent on macOS, a systemd user unit on Linux, or a logon task on Windows. It serves a mobile web app over HTTPS on your local network and, when Tailscale runs on the computer, over Tailscale. It keeps running when Sheprd quits, so phones keep working and notifications keep coming; pairing a new phone still needs Sheprd open.

The phone reaches your computer directly at its network addresses; there is no account to create. To reach it from anywhere without Tailscale, [relays](relays.md) are coming; you can already run your own. The daemon never stops or upgrades your Herdr daemon, and never starts Herdr 1; without one running, the phone shows that Herdr is not running. On a machine with Herdr 2 (herdr-core) and no Herdr 1, such as a cloud machine, opening the phone, or asking for a new tab or chat, starts herdr-core when it is not running.

From the phone you can:

- see your workspaces and their tabs on every session of this computer,
- follow [chats](chat.md) and send prompts,
- answer an agent's questions and permission requests,
- start a new chat or a new workspace,
- open a terminal tab and type into it.

## Turning it on

Open **Settings > Remote control** and turn on **Remote control**, or run:

```sh
sheprd config set remote.enabled true
```

The same page shows whether the service is running, on which port, and how many devices are paired and connected. It also holds the settings below, which are saved under `[remote]`:

```toml
[remote]
enabled = false
port = 7443
# Every interface ("all"), or one interface's address.
bind = "all"
# The daemon's own certificate authority, trusted once on the phone ("own"),
# or a certificate and key of your own:
# certificate = { cert = "/path/to/cert.pem", key = "/path/to/key.pem" }
certificate = "own"
# Keep serving phones, and sending notifications, after Sheprd quits.
keep_running = true
# "generic" notifications say which chat needs you, without its text;
# "excerpt" adds a short excerpt of the message.
notifications = "generic"
# Forget a paired device that has not connected for this many days (1 to 365),
# or 0 to keep it until it is revoked.
device_idle_days = 30
```

Device keys and the certificate authority stay in the daemon's own state directory, never in the config file. **Restart the daemon** starts it again from the binary beside Sheprd, as after an update; phones reconnect on their own.

## Pairing a phone

Choose **Pair a phone…** in Settings > Remote control, the **Pair phone** button in the title bar, or **Remote Control: Pair Device** in the command palette. The dialog shows a QR code that is valid for 60 seconds, can be used once, and is replaced while the dialog stays open.

1. Scan the code with the phone's camera. The page opens and asks for nothing.
2. The phone and the desktop both show the same 6-digit code, and the desktop asks: "*iPhone* wants remote control of your chats."
3. Choose **Allow** only if the phone shows the same code. **Deny**, a timeout, or a second scan of the same code voids the pairing.

Both sides prove who they are. The phone trusts the computer through the daemon's own certificate authority, whose fingerprint is in the QR code and shown on both screens. The computer trusts the phone through a key the phone creates at pairing, which cannot be exported. Revoke a phone any time in Settings > Remote control: its sessions end and its notifications stop at once.

### Choosing an address

The QR code prefers a Tailscale address when one is available, then a local network address. **Other addresses** lists the rest, each labelled with its network (Tailscale, Local network, Network bridge) and the interface it is on. Use Up and Down to change the address the QR code uses.

To pair across networks, connect both devices to the same Tailscale network and allow access to the remote-control port (7443 by default). Local addresses still work on the same network. Pair and install the web app from the stable Tailscale address, so it does not depend on a temporary Wi-Fi address.

## The web app

The pairing page offers the certificate authority to install on the phone. Pairing works without it, but the installed app and its notifications need it. The certificate authority covers the computer's addresses on your network and over Tailscale, and renews its certificates on its own, so you trust it once.

Add the page to your Home Screen to install it as an app. On iOS, Safari and the Home Screen app keep separate storage; the paired browser hands its pairing over to the installed app without a second approval. An unpaired Home Screen app can also scan a fresh pairing code directly.

Workspace headers show their pull request's number and status color, as on the desktop, for Herdr 2 workspaces too. Tapping the badge opens the pull request. No GitHub credentials are sent to the phone.

## Notifications

The installed app gets Web Push notifications when an agent asks a question or a permission, when a turn ends, and when an agent stops on an error. An agent in a plain terminal tab, Herdr 1 or Herdr 2, notifies too when it starts waiting for you and when it finishes what it was doing, once that status has held for two seconds; tapping it opens the terminal. On iOS 16.4 and later, push reaches only an installed app (added to the Home Screen); on Android it works installed or not.

By default a notification includes a short excerpt of the message, which can show on your phone's lock screen. To say only which chat needs you, without its text, choose **Generic** under **Notifications** in Settings, or set `notifications = "generic"` under `[remote]`. Notifications are encrypted end to end, so the push services relay them without reading them.

The app icon's badge counts agents that want you: those with a question, those Herdr reports blocked, and those that finished and nobody has opened yet. Phone notifications come from the daemon, so muting a workspace or tab in Sheprd does not hold them back.

## Your other machines

Allowing a phone can also pair it directly with the SSH and cloud machines Sheprd connects to. The Allow step lists them under "Also pair it with these machines, so it reaches them while this computer sleeps", each ticked. For each machine you keep, Sheprd installs and starts `sheprd-remote-control` there over SSH and adds the phone. The machine needs no internet access for this. Sheprd also puts `sheprd-acp` beside the daemon, so chats started from the phone work there: its own on a Mac paired from a Mac, and the release's static build, checked against the release's signature, on a Linux machine. Without one there (another kind of machine, or a development build), install it yourself (see [Chats on SSH hosts](chat.md#chats-on-ssh-hosts)); the daemon finds it in `~/.herdr/bin`, `~/.cargo/bin`, `~/.local/bin`, or the `PATH`.

The phone then reaches each machine's own daemon directly, so it keeps them while this computer sleeps. A machine added later is offered on the phone with a **Pair** button, and Sheprd asks on the desktop first. Revoking the phone here also revokes it on every machine it was paired with from here.

A machine is reachable from the phone only at a local network, Tailscale, or `.local` address. A machine reached only through a provider's proxy is paired but cannot be reached from the phone.

## iPhone and iPad app

> **TestFlight beta.** The native app for iPhone and iPad needs iOS 26 and is in beta. Builds can change or break between updates.

The app shows which agent needs you, lets you answer it, read its chat, and open its terminal. It pairs with Sheprd on your computer; nothing runs on the phone itself.

1. Install Apple's TestFlight app from the App Store.
2. Open [sheprd.org/testflight](https://sheprd.org/testflight/) on the device, tap **Join the TestFlight beta**, then **Accept** and **Install** in TestFlight.
3. In Sheprd on your computer, turn on remote control and open pairing. Scan the QR code with the app, or keep the phone nearby to pair over Bluetooth.

On macOS, pairing over Bluetooth needs Sheprd allowed in System Settings > Privacy & Security > Bluetooth. **Hold phone alerts while I'm here** in Settings > Remote control keeps a paired iPhone's alerts while it is near this computer over Bluetooth, until you step away. Nearby pairing and Live Activities are experimental.

## Command line

| Command | What it does |
| --- | --- |
| `sheprd remote` | Whether remote control is on and running: port, version, addresses, certificate authority fingerprint, and paired devices |
| `sheprd remote devices` | Paired devices with their IDs, last seen, notifications, and the other machines each was paired with |
| `sheprd remote pair [--address HTTPS_ORIGIN]` | Open the pairing dialog; you approve the phone there |
| `sheprd remote restart` | Restart the daemon from the binary beside the app |
| `sheprd remote revoke DEVICE` | Ask in the window to revoke a device; you confirm there |

While remote control is off, `remote` says so and exits 0; the other commands exit 1. `--address` takes an exact origin from `remote status`, such as `https://100.101.102.103:7443`.

```sh
sheprd remote status --json
sheprd remote revoke d_6f1c
```
