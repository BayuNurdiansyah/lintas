# lintas

Share one mouse and keyboard across Linux machines over LAN (like Lan Mouse / Barrier / Deskflow), but zero-config and compositor-agnostic. Repo: github.com/BayuNurdiansyah/lintas. License MIT. Single Rust binary, only dependency is the `evdev` crate.

## Why it exists

Lan Mouse and similar tools are painful to set up on Arch/CachyOS, mostly because Wayland input differs per compositor. lintas bypasses the compositor entirely:
- Capture: evdev, reading `/dev/input/event*`, with `EVIOCGRAB` so input doesn't leak to the host while controlling the remote.
- Injection: uinput virtual devices.

Works on X11, any Wayland compositor, and TTY. The only requirement is access to `/dev/input` and `/dev/uinput`, handled once by `packaging/install.sh` (udev rule + `input` group + uinput module).

## Positioning vs similar projects

- rkvm (htrefil/rkvm) uses the same evdev+uinput approach and has TLS, but switches by hotkey only and needs config files and certs.
- Lan Mouse / Deskflow / Input Leap have edge switching but through per-compositor integrations.
- lintas's differentiator: edge switching + monitor detection from the kernel + no config + works on any compositor. Don't claim "first ever"; claim "zero-config, compositor-agnostic edge switching".

## Current architecture (src/main.rs)

- `lintas serve [--port N] [--width PX] [--height PX] [--no-warp]` on the machine being controlled.
- `lintas host <ip[:port]> [--side left|right] [--width PX] [--height PX] [--speed F] [--no-warp]` on the machine that owns the mouse/keyboard.
- Host grabs all keyboards and mice (skips ABS devices like touchpads, and skips its own virtual devices named `lintas*`). Events go either to a local virtual device or over TCP to the remote.
- Wire protocol: TCP, `TCP_NODELAY`, fixed 8-byte messages (type u16, code u16, value i32, big endian). Real evdev events plus control messages:
  - `T_HELLO` `0xFFF0` (serve -> host on connect): code = screen height, value = screen width.
  - `T_ENTER` `0xFFF1` (host -> serve): code = entry edge (0 = right, 1 = left), value = y ratio * 1_000_000.
- Screen size is detected from `/sys/class/drm` (connected + enabled connectors, preferred mode): widths summed, tallest height used. Overridable with `--width`/`--height`.
- Cursor position (x and y) is dead-reckoned from relative motion and clamped at the screen walls, so the estimate resyncs whenever the cursor hits a wall. Crossing requires pushing `PUSH` (25) units past the edge.
- On switching, all held keys are released on the old target to prevent stuck keys. If the connection drops, control falls back to local.
- Cursor placement on entry uses a virtual pen tablet (`ABS_X`/`ABS_Y` + `BTN_TOOL_PEN` proximity in/out) so the cursor appears at the exact edge and at the same relative height, avoiding pointer acceleration. `--no-warp` falls back to a huge relative motion that snaps the cursor to the edge.
- Hotkeys on host: Ctrl+Alt+Shift+Space manual switch, Ctrl+Alt+Shift+Esc emergency exit.

## My setup

- Host: CachyOS PC with 2 monitors.
- Serve: laptop running Kali Linux, physically on the LEFT of the PC. Command: `lintas host <laptop-ip> --side left`.
- Laptop touchpad stays local on the laptop (not captured), that's intended.

## Status

- Done and tested on real hardware: hotkey switching, edge switching back and forth works.
- Just implemented, NOT yet tested on my desktop: preserving cursor height + virtual tablet placement. Compositors map tablets differently, so on a multi-monitor host the cursor might land on the wrong monitor. If so, investigate and fix (or fall back to `--no-warp` behavior on the host).
- Known issue earlier: when entering the laptop, the cursor used to appear at the height where it was last left instead of matching the PC's height. The tablet placement is meant to fix this.

## Roadmap

1. Verify/fix cursor placement on my setup.
2. Phase 2: mDNS auto-discovery, 6-digit pairing code, encryption (TLS or QUIC), systemd autostart.
3. Phase 4: settings UI for monitor/device layout + tray icon (Tauri or Slint).
4. Later: clipboard sync, touchpad capture on host, more than two machines, AUR/AppImage packaging.

## Rules

- All code comments, log messages, README, and docs in English (global audience).
- Before every commit: `cargo fmt`, `cargo clippy --release -- -D warnings`, `cargo build --release`. CI in `.github/workflows/ci.yml` runs the same checks and releases binaries on `v*` tags.
- Keep it a single lightweight binary; avoid heavy dependencies unless a phase needs them.
- Always keep an emergency way out when testing input grabbing.

## How to talk to me

Reply to me in casual Indonesian, use "aku" not "saya", be direct and concise, and never use em dashes. Code and docs stay in English.
