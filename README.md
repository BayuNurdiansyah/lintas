<p align="center">
  <img src="assets/logo-primary.png?v=3" width="120" alt="lintas logo">
</p>

<h1 align="center">lintas</h1>

<p align="center">
  Share one mouse and keyboard across Linux machines over your LAN.<br>
  Zero config. Any compositor. Push past the edge, keep going.
</p>

<p align="center">
  <img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue">
  <img alt="Platform: Linux" src="https://img.shields.io/badge/platform-Linux-lightgrey">
  <img alt="Built with Rust" src="https://img.shields.io/badge/built%20with-Rust-orange">
  <img alt="CI" src="https://github.com/BayuNurdiansyah/lintas/actions/workflows/ci.yml/badge.svg">
</p>

> **Status: alpha.** Used daily by the author. Expect rough edges.

*Lintas* is Indonesian for "to cross". Sit two Linux computers next to each other, push your cursor past the edge of one screen, and it continues onto the next machine, mouse and keyboard both.

---

## Contents

[Why](#why) · [Features](#features) · [How it compares](#how-it-compares) · [How it works](#how-it-works) · [Requirements](#requirements) · [Install](#install) · [Usage](#usage) · [Configuration](#configuration) · [Pairing and security](#pairing-and-security) · [Autostart](#autostart-on-login-systemd) · [Limitations](#known-limitations) · [Roadmap](#roadmap) · [License](#license)

## Why

Most mouse/keyboard sharing tools for Linux (Barrier, Synergy, Input Leap) read input through the display server. That breaks the moment you switch compositors, since every Wayland compositor exposes its own input hooks differently, and some expose none at all.

lintas skips the display server entirely. It reads raw input straight from the kernel (`evdev`) and injects it the same way (`uinput`). One consequence: it does not care what's drawing your desktop.

## Features

- **Works everywhere on Linux.** X11, any Wayland compositor, even a bare TTY with no graphical session. Nothing to configure per compositor.
- **Actually zero config.** One command per machine and you're done. A config file exists for convenience, not because you need it.
- **Multi-monitor edge switching, out of the box.** Layout is read from the kernel (`/sys/class/drm`), accurate even before you've logged into a desktop.
- **Cursor keeps its height when it crosses.** Placed at an exact pixel via a virtual drawing tablet, not reset to a corner or thrown off by pointer acceleration.
- **Encrypted, mutually authenticated, no CA to manage.** Each machine self-signs a certificate. First contact shows a short code on both screens; confirm once and it's remembered (TOFU). Both directions verify each other, an impersonating device can't inject input just by reaching your network.
- **Clipboard sync**, over the same encrypted connection.
- **Finds itself on the network** (mDNS), with a one-line manual fallback when it can't.
- **Optional tray icon and settings window, optional at compile time too.** Skip them entirely for a ~3MB headless build; see [Install](#install).
- **Single self-contained binary.** No interpreter, no runtime, no background services you didn't ask for.

## How it compares

| | lintas | rkvm | Lan Mouse / Deskflow / Input Leap |
|---|---|---|---|
| Reads input at | kernel (evdev) | kernel (evdev) | display server |
| Compositor-agnostic | ✅ | ✅ | per-compositor integration |
| Edge switching | ✅ | ❌ (hotkey only) | ✅ |
| Config required | none | file + certs | varies |
| Encrypted | ✅ TLS, mutual | ✅ TLS | varies |
| Auto-discovery | ✅ mDNS | ❌ | varies |

[rkvm](https://github.com/htrefil/rkvm) · [Lan Mouse](https://github.com/feschber/lan-mouse) · [Deskflow](https://github.com/deskflow/deskflow) · [Input Leap](https://github.com/input-leap/input-leap)

## How it works

1. The machine you control *from* (the **host**) grabs every physical keyboard, mouse, and touchpad with `evdev`, so nothing else on that machine sees that input while it's grabbed.
2. Events go to a local virtual device while the cursor is local, or over an encrypted connection to the other machine (**serve**) once the cursor crosses an edge.
3. `serve` injects those events with `uinput`, a virtual keyboard/mouse the kernel treats exactly like a physical one.
4. Crossing back releases any held keys first, so nothing gets stuck.
5. If the connection drops, control falls back to local automatically.

## Requirements

Linux on both machines. No Windows or macOS support, this depends directly on `evdev`/`uinput`.

Works on any distribution and any desktop environment or window manager: any X11 desktop, any Wayland compositor, or a bare TTY.

You need read access to `/dev/input/event*` and write access to `/dev/uinput`. The install steps below set that up.

## Install

Pick one. Do it on every machine you'll use lintas on.

<details open>
<summary><b>Option 1: build from source</b> (any distro, needs Rust)</summary>

```bash
# Install Rust if needed
sudo pacman -S rust                              # Arch based
sudo apt install rustc cargo                      # Debian/Ubuntu based
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # any distro, via rustup

git clone https://github.com/BayuNurdiansyah/lintas.git
cd lintas

# One-time: grants /dev/input + /dev/uinput access, opens firewall ports if ufw/firewalld is active
bash packaging/install.sh
# Log out and back in (or reboot) so the new group membership takes effect

cargo build --release
cp target/release/lintas ~/.local/bin/
```

Make sure `~/.local/bin` is on your `PATH`:

```bash
export PATH="$HOME/.local/bin:$PATH"   # add to ~/.bashrc or ~/.zshrc if missing
```

**Want the smallest build?** `cargo build --release --no-default-features` drops the optional GUI/tray toolkit, ~20MB down to ~3MB. Core `serve`/`host`/clipboard/pairing never depends on either.

</details>

<details>
<summary><b>Option 2: Arch Linux and derivatives</b> (CachyOS, Manjaro, EndeavourOS)</summary>

```bash
git clone https://github.com/BayuNurdiansyah/lintas.git
cd lintas/packaging
makepkg -si
```

Installs the binary, udev rule, both systemd units, desktop entry, and icon. Afterward it prints a reminder to run this once (a package script can't do it safely for you):

```bash
sudo usermod -aG input $USER
echo uinput | sudo tee /etc/modules-load.d/lintas.conf
sudo modprobe uinput
```

Log out and back in. You still need to open the firewall yourself here (see [Usage](#usage)), `makepkg` doesn't run `install.sh`.

</details>

<details>
<summary><b>Option 3: AppImage</b> (no install, any distro)</summary>

Grab `lintas-x86_64.AppImage` from [releases](https://github.com/BayuNurdiansyah/lintas/releases), or build it:

```bash
git clone https://github.com/BayuNurdiansyah/lintas.git
cd lintas
bash packaging/build-appimage.sh
```

Then:

```bash
chmod +x lintas-x86_64.AppImage
./lintas-x86_64.AppImage serve
```

Bundles its own GUI libraries, but still can't grant itself `/dev/input`/`/dev/uinput` access, so it needs the same one-time setup as Option 1 or 2.

</details>

## Usage

`serve` on the machine you're controlling; `host` on the machine with your mouse and keyboard.

```bash
# On the machine being controlled
lintas serve

# On the machine with your mouse/keyboard (controlled machine is on your left)
lintas host --side left
```

With no address, `host` looks for `serve` automatically via mDNS. If it finds nothing in a few seconds, your machines are likely on different subnets/VLANs (mDNS doesn't cross those by design, common with WiFi vs wired through some routers). Connect directly instead:

```bash
lintas host 192.168.1.50 --side left
```

or set it once via `lintas settings` (opens a small window) or `~/.config/lintas/config.toml` directly, see [Configuration](#configuration).

The first connection between two machines asks you to confirm a pairing code, see [Pairing and security](#pairing-and-security).

| Flag | Where | What |
|---|---|---|
| `--side left\|right` | host | Which side the other machine sits on. Default `left`. |
| `--width PX` | both | Override auto-detected total monitor width. |
| `--height PX` | both | Override auto-detected tallest monitor height. |
| `--speed F` | host | How far the cursor pushes before crossing, e.g. `0.8` or `1.3`. |
| `--no-warp` | both | Snap to the edge only, skip exact cursor placement. |
| `--tray` | both | System tray icon with connection status + Quit (needs a StatusNotifierItem host). |

**Hotkeys** (host, always on): `Ctrl+Alt+Shift+Space` manual switch · `Ctrl+Alt+Shift+Esc` emergency exit, releases all input.

**Firewall:** TCP `4242` (the connection itself), plus UDP `5353` for mDNS if you want auto-discovery. `install.sh` opens both automatically on ufw/firewalld; open them yourself otherwise.

## Configuration

Any flag above can be a default in `~/.config/lintas/config.toml`, a flag always wins over it.

```bash
lintas settings   # small window, same fields
```

or edit directly:

```toml
peer = "192.168.1.50:4242"
side = "left"
width = 3840
height = 1080
speed = 1.0
no_warp = false
tray = true
```

`peer` lets `lintas host` run alone, no address typed, no mDNS wait.

## Pairing and security

Pairing is mutual: `host` confirms it's talking to the right `serve`, and `serve` confirms it's accepting input from a `host` it recognizes, not from anything else on the network.

On first contact, both sides show a short code derived from the other's certificate. Confirm they match on both screens and it's remembered (`host` in `~/.local/share/lintas/trusted_peers`, `serve` in `~/.local/share/lintas/trusted_hosts`). A previously paired machine presenting a different certificate gets refused outright, not silently re-trusted, remove its entry from the relevant file to intentionally re-pair.

All traffic, input and clipboard both, is encrypted over this same authenticated connection.

## Autostart on login (systemd)

```bash
# serve
mkdir -p ~/.local/bin ~/.config/systemd/user
cp target/release/lintas ~/.local/bin/
cp packaging/lintas-serve.service ~/.config/systemd/user/
systemctl --user enable --now lintas-serve
```

Pair manually at least once first (run `serve`/`host` in a terminal, see [Usage](#usage)), a background service has no terminal for the pairing prompt.

```bash
# host (set peer + side in config.toml first, see Configuration)
mkdir -p ~/.local/bin ~/.config/systemd/user
cp target/release/lintas ~/.local/bin/
cp packaging/lintas-host.service ~/.config/systemd/user/
systemctl --user enable --now lintas-host
```

AUR install already places these files, just `systemctl --user enable --now` them.

## Known limitations

- **mDNS doesn't cross subnets/VLANs.** Multicast networking, not something lintas can fix in software. Use `peer` in the config or a direct address.
- **Touchpad gestures aren't captured, only basic pointer movement and clicks.** Multi-finger scroll/zoom/swipe gestures are handled by your desktop's own driver stack (libinput and friends), which lintas bypasses entirely by design; only single-finger position and physical clicks are forwarded.
- **Two machines only, for now.** One `serve`, one `host`.

## Roadmap

- [x] evdev → uinput forwarding, hotkey switching
- [x] Edge switching, multi-monitor detection, cursor height preserved
- [x] Mutual TLS encryption and pairing
- [x] systemd autostart
- [x] mDNS auto-discovery
- [x] Clipboard sync
- [x] System tray icon
- [x] Settings window
- [x] AUR package and AppImage
- [x] Touchpad capture on the host (pointer movement and clicks; no multi-finger gestures)
- [ ] More than two machines

## License

MIT
