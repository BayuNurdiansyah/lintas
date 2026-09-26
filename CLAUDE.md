# lintas

Share one mouse and keyboard across Linux machines over LAN (like Lan Mouse / Barrier / Deskflow), but zero-config and compositor-agnostic. Repo: github.com/BayuNurdiansyah/lintas. License MIT. Single Rust binary; deps: `evdev`, plus `rustls`/`rcgen`/`ring` for the TLS/pairing layer.

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

### Encryption + pairing (src/tls.rs)

- The wire protocol runs over TLS (rustls, ring crypto provider). No CA: each machine generates its own self-signed cert on first run via `rcgen` and persists it at `~/.local/share/lintas/{cert,key}.der`, so its fingerprint (and pairing code) stays stable across runs.
- `serve` presents its cert (`server_config`) and doesn't authenticate the host at all (`with_no_client_auth`). `host` connects with a client config whose certificate verifier (`AcceptAnyServerCert`) skips CA validation but still cryptographically verifies the handshake signature (`rustls::crypto::verify_tls{12,13}_signature`) — a live MITM within the session is still caught, only the "who issued this cert" check is skipped.
- Trust-on-first-use: `host` computes the SHA-256 fingerprint of the serve's cert (`fingerprint`), derives a 6-digit `pairing_code` from its first 4 bytes, and on first contact with a peer address prompts on stdin ("Does this match the code shown on `<peer>`'s screen? [y/N]") before storing the fingerprint in `~/.local/share/lintas/trusted_peers` (`tls::confirm_pairing`/`remember`/`check`). `serve` prints its own pairing code once at startup so the two can be compared by eye.
- If a peer's fingerprint ever differs from what's on file (`Trust::Changed`), the connection is refused with an explicit error rather than silently re-pairing — the user has to delete the stale entry from `trusted_peers` to re-pair intentionally.
- **Critical ordering constraint, learned the hard way**: pairing a new peer can block on a stdin prompt (`confirm_pairing`'s `interactive` path). `host` must always finish its first `connect(true)` *before* it grabs any input device. Once devices are grabbed (`EVIOCGRAB`), the keyboard driving that very terminal is captured too — a stdin prompt at that point can never be answered and freezes all input on the machine (mouse and keyboard both dead) until a hard reboot. `Host::connect` takes an `interactive` flag for this reason: `true` only for the pre-grab call in `host()`; `go_remote()`'s reconnect always passes `false` and fails closed (no prompt) if the peer isn't already paired.
- `Host.remote` and the per-connection `serve` socket are `tls::ClientStream`/`tls::ServerStream` (`rustls::StreamOwned<Connection, TcpStream>`), which own the socket and implement `Read`/`Write` directly — used as a drop-in replacement for the old raw `TcpStream` everywhere else in the code.
- Unit tests in `src/tls.rs` cover a real loopback TLS handshake (fingerprint match, encrypted round-trip) plus fingerprint/pairing-code sanity — run with `cargo test --release`, also wired into CI.

### Auto-discovery (src/discover.rs)

- `serve` advertises itself over mDNS (`mdns-sd` crate) as `_lintas._tcp.local.`, instance name = hostname (`discover::advertise`); the `ServiceDaemon` it returns must stay alive for the lifetime of `serve()` — dropping it stops the responder thread. Manually verified with `avahi-browse -r _lintas._tcp`.
- `lintas host` with no positional `<ip>` (or only flags) browses for `_lintas._tcp.local.` for 3 seconds (`discover::find_peer`), auto-picks if exactly one is found, otherwise numbers them and prompts on stdin. This prompt runs at the same pre-grab point as the pairing prompt, so it's safe by the same reasoning.
- Falls back cleanly: `lintas host <ip>` still works exactly as before, unaffected by discovery. If nothing is found, the error suggests using an explicit IP.
- Manually tested end to end on this machine: `serve` advertised correctly (confirmed via `avahi-browse`), `host` discovered it, listed multiple resolved addresses when more than one interface answered, and after picking one, paired and connected successfully with the pairing code matching what `serve` printed.

## My setup

- Host: CachyOS PC with 2 monitors.
- Serve: laptop running Kali Linux, physically on the LEFT of the PC. Command: `lintas host <laptop-ip> --side left`.
- Laptop touchpad stays local on the laptop (not captured), that's intended.

## Status

- Done and tested on real hardware (CachyOS 2-monitor host + Kali laptop serve): hotkey switching, edge switching back and forth, and cursor height + virtual tablet placement all work correctly, including landing on the right monitor on the 2-monitor host.
- TLS encryption + pairing code (see `src/tls.rs` above) and systemd autostart for both `serve` and `host` (`packaging/lintas-serve.service`, `packaging/lintas-host.service` + `lintas-host.env.example`) are implemented and pass unit tests / clippy / release build.
- Real-hardware test of pairing initially hit a serious bug: the host froze all keyboard/mouse input machine-wide (had to hard reboot) because the pairing prompt was asked *after* devices were already grabbed, so the keyboard needed to answer it had already been captured exclusively by lintas. Fixed by moving the first `connect(true)` before the device-grab loop and making all later reconnects non-interactive (see the ordering note in the Encryption section above). Re-tested on the real laptop + PC setup and confirmed working: pairing prompt answerable, no freeze.
- mDNS auto-discovery (see `src/discover.rs` above) is implemented and tested on this machine (single-machine loopback + local interface), but **not yet tested across the real laptop + PC pair on the actual LAN**.
- Phase 2 is functionally complete; only real cross-machine testing of mDNS discovery and of the systemd unit files remains.

## Roadmap

1. ~~Verify/fix cursor placement on my setup.~~ Done, confirmed working.
2. Phase 2:
   - [x] Encryption (TLS) + 6-digit pairing code — implemented and confirmed working on real hardware (see Status for the freeze bug that got fixed along the way).
   - [x] systemd autostart for `serve` and `host` — implemented, untested on real hardware.
   - [x] mDNS auto-discovery — implemented, tested on one machine, needs a real cross-machine test (laptop discovering the PC or vice versa).
3. Phase 4: settings UI for monitor/device layout + tray icon (Tauri or Slint).
4. Later: clipboard sync, touchpad capture on host, more than two machines, AUR/AppImage packaging.

## Rules

- All code comments, log messages, README, and docs in English (global audience).
- Before every commit: `cargo fmt`, `cargo clippy --release -- -D warnings`, `cargo build --release`, `cargo test --release`. CI in `.github/workflows/ci.yml` runs the same checks and releases binaries on `v*` tags.
- Keep it a single lightweight binary; avoid heavy dependencies unless a phase needs them (the TLS stack was an explicit exception for the Phase 2 encryption item).
- Always keep an emergency way out when testing input grabbing.

## How to talk to me

Reply to me in casual Indonesian, use "aku" not "saya", be direct and concise, and never use em dashes. Code and docs stay in English.
