# lintas

*Lintas* is the Indonesian word for "to cross" or "to pass across". This tool moves your mouse cursor and keyboard input across from one Linux machine to another, the same way Barrier, Synergy, Lan Mouse, or Deskflow do, but with a different design goal: it should work with zero setup and it should not care which compositor or display server your machines run.

Sit two (or more) Linux computers next to each other on the same network, and control both with a single mouse and keyboard. Push your cursor past the edge of one screen, and it continues onto the next machine.

> Status: **alpha**. It works and is used daily by the author, but expect rough edges.

## What makes it different

Most mouse and keyboard sharing tools for Linux read input through the display server (X11 or a Wayland protocol). That approach breaks the moment you switch compositors, because every Wayland compositor implements its own input protocols differently, and some do not expose the hooks these tools need at all.

lintas skips the display server entirely. It reads raw input events straight from the kernel (`evdev`, `/dev/input/event*`) and injects them the same way (`uinput`). Because of that:

- **It works everywhere on Linux.** X11, any Wayland compositor (GNOME, KDE, Hyprland, Sway, and so on), and even a plain TTY console with no graphical session at all. There is nothing to configure per compositor.
- **No config file is required to get started.** Run one command on each machine and you are done. A config file exists for convenience, not because you need one.
- **Edge switching works out of the box, including with multiple monitors.** Screen layout is read straight from the kernel (`/sys/class/drm`), not from a display server, so it is accurate even before you have logged into a desktop session.
- **The cursor keeps its vertical position when it crosses over.** Most tools reset the cursor to a fixed spot or lose precision because of pointer acceleration. lintas places the cursor at an exact pixel position using a virtual drawing tablet device, so it lands exactly where you would expect.
- **Traffic is encrypted, with no certificate authority to manage.** Each machine generates its own certificate on first run. The first time two machines connect, both show a short numeric code. If the two codes match, you confirm it once and the machines remember each other from then on (trust on first use). If a machine's certificate ever changes unexpectedly, the connection is refused instead of silently trusting it again, which protects against a machine being swapped or impersonated on the network.
- **Clipboard sync.** Copying text on one machine makes it available for pasting on the other, over the same encrypted connection.
- **Automatic discovery on the local network (mDNS), with a manual fallback.** You are not required to know or type an IP address. If mDNS does not reach across your network (see the note in Usage below), you can set an address once and never think about it again.
- **A small system tray icon and a settings window are both optional, not required, and built as optional at the compile level too.** They pull in a GUI toolkit and a D-Bus stack that core `serve`/`host`/clipboard/pairing functionality never touches. The default build includes both for convenience (about 20MB), but `cargo build --release --no-default-features` drops that to about 3MB by cutting them out entirely, for anyone who only wants the headless CLI, for example a minimal server-only install. CI builds and checks both configurations on every change.
- **A single, self-contained binary.** No interpreter, no runtime, no background services beyond what you explicitly enable.

## How it works, briefly

1. The machine you want to control from (called the **host**) grabs every physical keyboard and mouse using `evdev`, so no other application on that machine sees that input while it is grabbed.
2. Input events are forwarded either to a virtual input device on the host itself (when the cursor is still local) or, once the cursor crosses a screen edge, over an encrypted TCP connection to the other machine (called **serve**).
3. The serve machine injects those events into its own system using `uinput`, a virtual keyboard and mouse the kernel treats exactly like a physical one.
4. When control crosses back and forth, any keys still held down are released first, so nothing gets stuck.
5. If the network connection drops, control automatically falls back to the local machine.

## Requirements

lintas is Linux only, on both machines. It does not run on Windows or macOS, because it depends directly on Linux kernel interfaces (`evdev` and `uinput`).

It works on any Linux distribution and any desktop environment or window manager, including:

- Any X11 desktop (GNOME, KDE, XFCE, Cinnamon, MATE, i3, and so on)
- Any Wayland compositor (GNOME, KDE, Hyprland, Sway, River, and so on)
- A bare TTY with no graphical session running at all

You need permission to read `/dev/input/event*` and to create devices through `/dev/uinput`. The install steps below set that up for you.

## Install

Pick whichever of the three methods below fits your setup. Do the install on every machine you plan to use lintas on.

### Option 1: build from source

Works on any Linux distribution. Requires the Rust toolchain.

```bash
# Install Rust if you don't already have it
# Arch based distros:
sudo pacman -S rust
# Debian/Ubuntu based distros:
sudo apt install rustc cargo
# Or on any distro, via rustup:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

git clone https://github.com/BayuNurdiansyah/lintas.git
cd lintas

# One time setup: grants access to /dev/input and /dev/uinput,
# and opens the firewall ports lintas needs if ufw or firewalld is active.
bash packaging/install.sh
# Log out and back in (or reboot) afterwards, so the new group
# membership takes effect.

cargo build --release
cp target/release/lintas ~/.local/bin/
```

Make sure `~/.local/bin` is in your shell's `PATH`. If `lintas` is not found after this, add the following to your shell's startup file (`~/.bashrc`, `~/.zshrc`, and so on) and restart your terminal:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

### Option 2: Arch Linux and Arch based distros (CachyOS, Manjaro, EndeavourOS)

`packaging/PKGBUILD` builds a package from the latest commit on the `main` branch.

```bash
git clone https://github.com/BayuNurdiansyah/lintas.git
cd lintas/packaging
makepkg -si
```

This installs the binary, the udev rule, both systemd service files, a desktop entry, and the application icon for you. After installing, it prints a one time reminder to add yourself to the `input` group and load the `uinput` kernel module, since a package installer cannot safely do that on its own:

```bash
sudo usermod -aG input $USER
echo uinput | sudo tee /etc/modules-load.d/lintas.conf
sudo modprobe uinput
```

Then log out and back in. You still need to open the firewall ports yourself on this path (see Firewall below), since `makepkg` does not run `install.sh`.

### Option 3: AppImage (no installation, any distro)

Download `lintas-x86_64.AppImage` from the [releases page](https://github.com/BayuNurdiansyah/lintas/releases), or build it yourself:

```bash
git clone https://github.com/BayuNurdiansyah/lintas.git
cd lintas
bash packaging/build-appimage.sh
```

Then make it executable and run it directly, no installation step needed:

```bash
chmod +x lintas-x86_64.AppImage
./lintas-x86_64.AppImage serve
```

The AppImage bundles the shared libraries the settings window needs, so it runs on most distributions without installing anything extra. It still cannot grant itself access to `/dev/input` and `/dev/uinput`, so you still need to run the same one time setup as in Option 1 or Option 2 on that machine (the `bash packaging/install.sh` step, or the manual `usermod`/`modprobe` commands above).

### Firewall

lintas needs TCP port `4242` open (the connection between the two machines) and, if you want automatic discovery, UDP port `5353` open as well (mDNS). `packaging/install.sh` opens both automatically if you have `ufw` or `firewalld` active. If you use a different firewall, or installed via the AUR package, open them yourself.

## Usage

Run `serve` on the machine you want to be controlled, and `host` on the machine that owns the mouse and keyboard you want to use.

```bash
# On the machine being controlled
lintas serve

# On the machine with your mouse and keyboard,
# with the controlled machine on your left:
lintas host --side left
```

With no address given, `host` looks for a `serve` machine automatically on the local network using mDNS, and asks you to confirm if it finds more than one. If it does not find anything within a few seconds, mDNS is likely not reaching across your network. This is common if your two machines are on different subnets or VLANs, for example one on Wi-Fi and one on a wired connection through a router that separates them, since mDNS by design does not cross subnet boundaries. In that case, connect by address directly instead:

```bash
lintas host 192.168.1.50 --side left
```

or save the address once so you never need to type it again:

```bash
lintas settings
```

which opens a small window to edit the same options described below, or edit `~/.config/lintas/config.toml` directly (see Configuration).

The first time two machines connect to each other, you will be asked to confirm a short pairing code shown on both screens (see Pairing below).

| Option | Where it applies | What it does |
|---|---|---|
| `--side left` or `--side right` | host | Which side the other machine sits on, relative to the one you are controlling from. Default is `left`. |
| `--width PX` | both | Total combined width of all monitors, in pixels, if you need to override automatic detection (for example with display scaling). |
| `--height PX` | both | Height of the tallest monitor, in pixels, if you need to override automatic detection. |
| `--speed F` | host | Adjusts how far the cursor needs to be pushed before it crosses over, for example `0.8` (more sensitive) or `1.3` (less sensitive). |
| `--no-warp` | both | Disables exact cursor placement on entry and only snaps the cursor to the screen edge instead. Use this if your compositor places the cursor on the wrong monitor. |
| `--tray` | both | Shows a small system tray icon with the current connection status and a Quit action. Requires a desktop that supports StatusNotifierItem tray icons (GNOME, KDE, and XFCE all do, with a tray applet enabled). |

Hotkeys, always active on the host: `Ctrl+Alt+Shift+Space` switches control manually between machines, and `Ctrl+Alt+Shift+Esc` is an emergency exit that immediately releases all input and quits.

## Configuration

Every option above can also be set as a default in a config file, at `~/.config/lintas/config.toml`, so you do not need to repeat flags every time. A command line flag always overrides the config file.

The easiest way to edit it is to run:

```bash
lintas settings
```

which opens a small window with all the same fields. Alternatively, copy `packaging/config.toml.example` and edit it by hand:

```toml
peer = "192.168.1.50:4242"
side = "left"
width = 3840
height = 1080
speed = 1.0
no_warp = false
tray = true
```

Setting `peer` here lets you run `lintas host` on its own, with no address and without waiting for mDNS discovery, since the address is already known.

## Pairing and security

Pairing works in both directions: `host` confirms it is talking to the right `serve` machine, and `serve` confirms it is accepting input from a `host` it recognizes, not from any other device on the network.

The first time the two machines connect to each other, each side displays a six digit code, derived from the other machine's certificate. Confirm on both screens that the codes shown match what the other machine displays, and pairing is remembered from then on: `host` stores it in `~/.local/share/lintas/trusted_peers`, `serve` in `~/.local/share/lintas/trusted_hosts`. If a previously paired machine ever presents a different certificate, for example because it was reinstalled or a different device is answering on that address, the connection is refused rather than silently trusted again. To intentionally re-pair such a machine, remove its entry from the relevant file and connect again.

All traffic between the two machines, both input events and clipboard content, is encrypted over this same connection, and both ends authenticate each other before any input is forwarded.

## Autostart on login (systemd)

To have `serve` start automatically when you log in:

```bash
mkdir -p ~/.local/bin ~/.config/systemd/user
cp target/release/lintas ~/.local/bin/
cp packaging/lintas-serve.service ~/.config/systemd/user/
systemctl --user enable --now lintas-serve
```

Before enabling either service for the first time, pair the two machines manually at least once by running `serve` and `host` directly in a terminal on each (see Usage above), since a background service has no terminal to confirm a pairing code with, on either side.

To do the same for `host`, first make sure `peer` and `side` are set in `~/.config/lintas/config.toml` (see Configuration above):

```bash
mkdir -p ~/.local/bin ~/.config/systemd/user
cp target/release/lintas ~/.local/bin/
cp packaging/lintas-host.service ~/.config/systemd/user/
systemctl --user enable --now lintas-host
```

If you installed via the AUR package, these service files are already placed for you, and only need `systemctl --user enable --now` to turn on.

## Known limitations

- **mDNS auto-discovery does not cross network subnets or VLANs.** This is a property of multicast networking in general, not something lintas can work around in software. If discovery does not find your other machine, set `peer` in the config file or pass an address directly, both described above.
- **Touchpad capture on the host is not implemented yet.** If the machine you are controlling from has a touchpad, it stays local to that machine and is not shared, which is usually what you want anyway.
- **Only two machines at a time for now.** One `serve` and one `host`. Support for more machines is planned but not built yet.

## Roadmap

- [x] Forward raw input from evdev to uinput, with hotkey switching
- [x] Edge switching, multi-monitor detection, cursor height preserved on crossing
- [x] Encryption and pairing code (TLS)
- [x] systemd autostart on both ends
- [x] Automatic discovery on the local network (mDNS)
- [x] Clipboard sync
- [x] System tray icon
- [x] Settings window for the config file
- [x] AUR package and AppImage
- [ ] Touchpad capture on the host
- [ ] More than two machines at once

## Similar projects

- [rkvm](https://github.com/htrefil/rkvm): the same evdev and uinput approach, with TLS, but switches only by hotkey and needs a hand written config file and certificates.
- [Lan Mouse](https://github.com/feschber/lan-mouse), [Deskflow](https://github.com/deskflow/deskflow), and [Input Leap](https://github.com/input-leap/input-leap): all support edge switching, but through integrations built for each specific compositor rather than reading input at the kernel level.

## License

MIT
