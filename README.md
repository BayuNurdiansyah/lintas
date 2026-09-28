# lintas

> *Lintas* means "to cross" in Indonesian. Move your cursor across machines. Zero config, any compositor.

Share one mouse and keyboard between Linux machines over your LAN. Push the cursor past the edge of your screen and it continues on the machine next to you.

- **Desktop agnostic.** Works on X11, Wayland (KDE, GNOME, Hyprland, Sway) and even a bare TTY, because input is read and injected at the kernel level (evdev + uinput).
- **Edge switching without compositor support.** Monitor layout is detected from the kernel (`/sys/class/drm`), so multi-monitor setups work out of the box.
- **Keeps your cursor height.** The cursor enters the other machine at the same relative height it left.
- **Minimal setup.** One binary, one permission script, no config file.
- **Encrypted.** Traffic is TLS-encrypted. There's no certificate authority: each machine generates its own self-signed cert, and the first connection to a new peer is confirmed with a 6-digit pairing code shown on both screens (trust-on-first-use, remembered afterwards).
- **Clipboard sync.** Copying text on either machine makes it available on the other, over the same encrypted connection.

> Status: **alpha**.

## Install

Requires Rust (`sudo pacman -S rust` on Arch, or [rustup](https://rustup.rs) elsewhere).

```bash
git clone https://github.com/BayuNurdiansyah/lintas.git
cd lintas
bash packaging/install.sh   # grants access to /dev/input and /dev/uinput, opens firewall ports if ufw/firewalld is active, then reboot
cargo build --release
cp target/release/lintas ~/.local/bin/
```

Do this on every machine.

## Usage

Example: laptop on the left, desktop PC on the right (the PC owns the mouse and keyboard).

```bash
# on the laptop
lintas serve

# on the PC
lintas host <laptop-ip> --side left
# or, to find it automatically over mDNS instead of typing an IP:
lintas host --side left
```

| Option | Where | Purpose |
|---|---|---|
| `--side left\|right` | host | Where the remote machine sits relative to the host (default `left`) |
| `--width PX` | both | Total width of all monitors, if auto-detection is off (e.g. with scaling) |
| `--height PX` | both | Height of the tallest monitor, if auto-detection is off |
| `--speed F` | host | Tune where the crossing triggers, e.g. `0.8` or `1.3` |
| `--no-warp` | both | Disable exact cursor placement and only snap to the edge |
| `--tray` | both | Show a system tray icon with connection status and a Quit action (requires a StatusNotifierItem host, e.g. GNOME/KDE/XFCE with a systray applet) |

Hotkeys: `Ctrl+Alt+Shift+Space` switches manually, `Ctrl+Alt+Shift+Esc` is an emergency exit.

### Config file

Any of the above can be set as a default instead of a flag, in `~/.config/lintas/config.toml` (copy `packaging/config.toml.example`, or edit it with `lintas settings`, a small native window). A CLI flag always overrides the config file. `host` can also set `peer` there so `lintas host` alone connects without typing an IP or waiting on mDNS.

Default port is TCP `4242`, plus UDP `5353` for mDNS discovery — `packaging/install.sh` opens both automatically if ufw or firewalld is active; otherwise open them manually.

### Pairing

The first time a host connects to a serve machine, both sides print a 6-digit pairing code derived from the serve's certificate. Confirm on the host that the codes match, and it's remembered from then on (in `~/.local/share/lintas/trusted_peers`). If a peer's certificate ever changes unexpectedly, the connection is refused instead of silently re-pairing.

To autostart on the serve side, see `packaging/lintas-serve.service`. To autostart on the host side, copy `packaging/lintas-host.service` to `~/.config/systemd/user/`, copy `packaging/lintas-host.env.example` to `~/.config/lintas/lintas-host.env` and fill in `LINTAS_PEER`/`LINTAS_SIDE`, then run `systemctl --user enable --now lintas-host`.

## How it works

1. The host grabs every keyboard and mouse (evdev) and forwards events either to a local virtual device or to the remote.
2. The cursor position is estimated from relative mouse motion and resynced every time it hits a screen wall.
3. When the cursor crosses the edge, events go to the remote, which injects them through uinput.
4. On entry the cursor is placed exactly using a small virtual pen tablet, which avoids pointer acceleration errors. If your compositor places it on the wrong monitor, use `--no-warp`.

## Roadmap

- [x] Forward evdev input to uinput, hotkey switching
- [x] Edge switching, multi-monitor detection, cursor height preserved
- [x] Pairing code, encryption (TLS)
- [x] systemd autostart on both ends
- [x] Auto-discovery (mDNS)
- [x] Clipboard sync
- [x] Tray icon (`--tray`)
- [x] Settings UI (`lintas settings`) for the config file
- [ ] Touchpad capture on the host
- [ ] More than two machines

## Similar projects

- [rkvm](https://github.com/htrefil/rkvm): same evdev + uinput approach with TLS, but switches via hotkey only and needs a config file.
- [Lan Mouse](https://github.com/feschber/lan-mouse), [Deskflow](https://github.com/deskflow/deskflow), [Input Leap](https://github.com/input-leap/input-leap): edge switching through per-compositor integrations.

## License

MIT
