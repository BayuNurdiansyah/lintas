//! lintas: share one mouse and keyboard across Linux machines over LAN.
//!
//! Input is captured with evdev and injected with uinput, so it works the same
//! on X11, any Wayland compositor, and even a bare TTY.
//!
//!   lintas serve [--port N] [--width PX] [--height PX] [--no-warp] [--tray]
//!   lintas host <ip[:port]> [--side left|right] [--width PX] [--height PX]
//!                           [--speed F] [--no-warp] [--tray]
//!
//! --side    : where the remote machine sits relative to the host (default: left)
//! --width   : total width of all monitors in px (default: auto-detected)
//! --height  : tallest monitor height in px (default: auto-detected)
//! --speed   : tune where the edge crossing triggers (default: 1.0)
//! --no-warp : disable exact cursor placement, only snap to the edge
//! --tray    : show a system tray icon with connection status and Quit
//!
//! Host hotkeys:
//!   Ctrl+Alt+Shift+Space  switch local <-> remote manually
//!   Ctrl+Alt+Shift+Esc    emergency exit (releases all input)

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{
    AbsInfo, AbsoluteAxisType, AttributeSet, Device, EventType, InputEvent, Key, RelativeAxisType,
    UinputAbsSetup,
};
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

mod clipboard;
mod config;
mod discover;
mod settings_gui;
mod tls;
mod tray;

const DEFAULT_PORT: u16 = 4242;
const VDEV_PREFIX: &str = "lintas";
/// How far the mouse must be pushed past an edge before switching.
const PUSH: f64 = 25.0;
/// Huge relative motion used to pin the cursor to an edge (the compositor clamps it).
const SLAM: i32 = 100_000;
/// Coordinate range of the virtual tablet used for exact cursor placement.
const ABS_MAX: i32 = 65_535;
/// Fixed-point scale for sending the vertical position as a ratio.
const RATIO_SCALE: f64 = 1_000_000.0;
/// How often the read side polls for new frames when there's nothing to
/// read, so clipboard sync (and, on `host`, incoming frames in general) get
/// a chance to run without waiting for actual input events.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

// Control messages, outside the range of real evdev event types.
/// serve -> host. code = screen height, value = screen width.
const T_HELLO: u16 = 0xFFF0;
/// host -> serve. code = entry edge (0 = right, 1 = left), value = y ratio.
const T_ENTER: u16 = 0xFFF1;
/// Either direction. code = unused, value = length of the UTF-8 clipboard
/// text that immediately follows this frame on the wire (not itself framed).
const T_CLIP: u16 = 0xFFF2;

/// A raw input event on the wire: (type, code, value).
type Ev = (u16, u16, i32);

/// Reassembles fixed 8-byte frames from a stream that may be polled with a
/// read timeout: a timeout preserves whatever partial frame was already
/// read, instead of losing it like a fresh `read_exact` call would.
#[derive(Default)]
struct FrameReader {
    buf: [u8; 8],
    filled: usize,
}

impl FrameReader {
    /// `Ok(None)` means "no complete frame yet" (e.g. the read timed out);
    /// call again later. `Err` means the connection is actually gone.
    fn poll(&mut self, stream: &mut impl Read) -> io::Result<Option<[u8; 8]>> {
        loop {
            match stream.read(&mut self.buf[self.filled..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "connection closed",
                    ))
                }
                Ok(n) => {
                    self.filled += n;
                    if self.filled == self.buf.len() {
                        self.filled = 0;
                        return Ok(Some(self.buf));
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None)
                }
                Err(e) => return Err(e),
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy)]
struct Screen {
    w: f64,
    h: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config = config::load();
    let warp = !args.iter().any(|a| a == "--no-warp") && !config.no_warp.unwrap_or(false);
    let show_tray = args.iter().any(|a| a == "--tray") || config.tray.unwrap_or(false);
    let res = match args.get(1).map(|s| s.as_str()) {
        Some("settings") => settings_gui::run(),
        Some("serve") => {
            let port = opt(&args, "--port")
                .and_then(|p| p.parse().ok())
                .or(config.port)
                .unwrap_or(DEFAULT_PORT);
            serve(port, screen_from_args(&args, &config), warp, show_tray)
        }
        Some("host") => {
            let peer = if args.len() > 2 && !args[2].starts_with("--") {
                Some(args[2].clone())
            } else {
                config.peer.clone()
            };
            let peer = match peer {
                Some(mut peer) => {
                    if !peer.contains(':') {
                        peer = format!("{peer}:{DEFAULT_PORT}");
                    }
                    Ok(peer)
                }
                None => discover::find_peer().map(|a| a.to_string()),
            };
            match peer {
                Ok(peer) => {
                    let side = match opt(&args, "--side").or(config.side.clone()).as_deref() {
                        Some("right") => Side::Right,
                        _ => Side::Left,
                    };
                    let speed = opt(&args, "--speed")
                        .and_then(|s| s.parse().ok())
                        .or(config.speed)
                        .unwrap_or(1.0);
                    host(
                        &peer,
                        side,
                        screen_from_args(&args, &config),
                        speed,
                        warp,
                        show_tray,
                    )
                }
                Err(e) => Err(e),
            }
        }
        _ => {
            eprintln!(
                "Usage:\n  lintas serve [--port N] [--width PX] [--height PX] [--no-warp] [--tray]\n  \
                 lintas host [ip[:port]] [--side left|right] [--width PX] [--height PX] \
                 [--speed F] [--no-warp] [--tray]\n  \
                 lintas settings\n\
                 Without an ip, lintas host looks for a lintas serve on the LAN via mDNS.\n\
                 Defaults for any of these can be set in ~/.config/lintas/config.toml \
                 (see packaging/config.toml.example), editable with `lintas settings`."
            );
            std::process::exit(1);
        }
    };
    if let Err(e) = res {
        eprintln!("Error: {e}");
        if e.kind() == io::ErrorKind::PermissionDenied {
            eprintln!("Run packaging/install.sh, then log out and back in.");
        }
        std::process::exit(1);
    }
}

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn screen_from_args(args: &[String], config: &config::Config) -> Screen {
    let detected = detect_screen();
    let w = opt(args, "--width")
        .and_then(|v| v.parse().ok())
        .or(config.width);
    let h = opt(args, "--height")
        .and_then(|v| v.parse().ok())
        .or(config.height);
    let s = Screen {
        // `.max(1.0)` guards against a bad --width/--height or config value
        // (zero, negative, or unparseable-but-slipped-through) later
        // panicking a `.clamp(min, max)` call elsewhere.
        w: w.unwrap_or(detected.w).max(1.0),
        h: h.unwrap_or(detected.h).max(1.0),
    };
    println!("Screen: {}x{} px", s.w, s.h);
    s
}

/// Detect the desktop size from the kernel (DRM), independent of X11/Wayland.
/// Assumes monitors are placed side by side: widths are summed, the tallest
/// height is used.
fn detect_screen() -> Screen {
    let (mut w, mut h) = (0u32, 0u32);
    if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("card") || !name.contains('-') {
                continue;
            }
            let p = e.path();
            let read = |f: &str| std::fs::read_to_string(p.join(f)).unwrap_or_default();
            if read("status").trim() != "connected" || read("enabled").trim() == "disabled" {
                continue;
            }
            // First line of `modes` is the preferred mode, e.g. "2560x1440"
            let modes = read("modes");
            let Some(mode) = modes.lines().next() else {
                continue;
            };
            let mut parts = mode.split('x');
            let mw = parts.next().and_then(|v| v.parse::<u32>().ok());
            let mh = parts.next().and_then(|v| {
                let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse::<u32>().ok()
            });
            if let (Some(mw), Some(mh)) = (mw, mh) {
                println!("Monitor {name}: {mw}x{mh}");
                w += mw;
                h = h.max(mh);
            }
        }
    }
    if w == 0 || h == 0 {
        println!("No monitor detected, assuming 1920x1080 (override with --width/--height)");
        return Screen {
            w: 1920.0,
            h: 1080.0,
        };
    }
    Screen {
        w: w as f64,
        h: h as f64,
    }
}

/// Virtual keyboard + relative mouse that can emit any key or button.
fn make_vdev(name: &str) -> io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<Key>::new();
    for code in 1..0x2ff {
        keys.insert(Key::new(code));
    }
    let mut rel = AttributeSet::<RelativeAxisType>::new();
    for r in [
        RelativeAxisType::REL_X,
        RelativeAxisType::REL_Y,
        RelativeAxisType::REL_WHEEL,
        RelativeAxisType::REL_HWHEEL,
        RelativeAxisType::REL_WHEEL_HI_RES,
        RelativeAxisType::REL_HWHEEL_HI_RES,
    ] {
        rel.insert(r);
    }
    VirtualDeviceBuilder::new()?
        .name(name)
        .with_keys(&keys)?
        .with_relative_axes(&rel)?
        .build()
}

/// Places the cursor at an exact position on the desktop.
///
/// Uses a virtual pen tablet: compositors map tablet coordinates onto the
/// desktop, so a short proximity-in/out moves the cursor there instantly,
/// without the pointer acceleration that makes relative jumps imprecise.
/// Falls back to snapping the cursor to an edge when disabled.
struct Placer {
    tablet: Option<VirtualDevice>,
}

impl Placer {
    fn new(name: &str, enabled: bool) -> Self {
        if !enabled {
            return Placer { tablet: None };
        }
        let tablet = (|| -> io::Result<VirtualDevice> {
            let mut keys = AttributeSet::<Key>::new();
            keys.insert(Key::BTN_TOOL_PEN);
            keys.insert(Key::BTN_TOUCH);
            keys.insert(Key::BTN_STYLUS);
            let axis = |a| UinputAbsSetup::new(a, AbsInfo::new(0, 0, ABS_MAX, 0, 0, 100));
            VirtualDeviceBuilder::new()?
                .name(name)
                .with_keys(&keys)?
                .with_absolute_axis(&axis(AbsoluteAxisType::ABS_X))?
                .with_absolute_axis(&axis(AbsoluteAxisType::ABS_Y))?
                .build()
        })();
        match tablet {
            Ok(t) => Placer { tablet: Some(t) },
            Err(e) => {
                eprintln!("Exact cursor placement unavailable ({e}), using edge snap");
                Placer { tablet: None }
            }
        }
    }

    /// Move the cursor to (fx, fy), both as fractions of the desktop (0.0..=1.0).
    /// `rel` is the relative device, used for the edge-snap fallback.
    fn place(&mut self, rel: &mut VirtualDevice, fx: f64, fy: f64) -> io::Result<()> {
        match self.tablet.as_mut() {
            Some(t) => {
                let x = (fx.clamp(0.0, 1.0) * ABS_MAX as f64) as i32;
                let y = (fy.clamp(0.0, 1.0) * ABS_MAX as f64) as i32;
                let key = EventType::KEY.0;
                let abs = EventType::ABSOLUTE.0;
                t.emit(&to_input(&[
                    (key, Key::BTN_TOOL_PEN.code(), 1),
                    (abs, AbsoluteAxisType::ABS_X.0, x),
                    (abs, AbsoluteAxisType::ABS_Y.0, y),
                ]))?;
                t.emit(&to_input(&[(key, Key::BTN_TOOL_PEN.code(), 0)]))
            }
            None => {
                let dir = if fx < 0.5 { -1 } else { 1 };
                rel.emit(&to_input(&[(
                    EventType::RELATIVE.0,
                    RelativeAxisType::REL_X.0,
                    dir * SLAM,
                )]))
            }
        }
    }
}

fn to_input(evs: &[Ev]) -> Vec<InputEvent> {
    evs.iter()
        .map(|&(t, c, v)| InputEvent::new(EventType(t), c, v))
        .collect()
}

pub(crate) fn encode(buf: &mut Vec<u8>, (t, c, v): Ev) {
    buf.extend_from_slice(&t.to_be_bytes());
    buf.extend_from_slice(&c.to_be_bytes());
    buf.extend_from_slice(&v.to_be_bytes());
}

fn decode(b: &[u8; 8]) -> Ev {
    (
        u16::from_be_bytes([b[0], b[1]]),
        u16::from_be_bytes([b[2], b[3]]),
        i32::from_be_bytes([b[4], b[5], b[6], b[7]]),
    )
}

// ---------------------------------------------------------------- SERVE

fn serve(port: u16, screen: Screen, warp: bool, show_tray: bool) -> io::Result<()> {
    let tray = show_tray.then(|| tray::spawn("Waiting for host")).flatten();
    let mut vdev = make_vdev(&format!("{VDEV_PREFIX}-remote"))?;
    let mut placer = Placer::new(&format!("{VDEV_PREFIX}-remote-placer"), warp);
    let identity = tls::load_or_create_identity()?;
    let tls_config = tls::server_config(&identity)?;
    let fp = tls::fingerprint(&identity.cert);
    println!(
        "This machine's pairing code: {:06}. A host connecting for the first time \
         must see the same code before you confirm the pairing.",
        tls::pairing_code(&fp)
    );
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    // Kept alive for the rest of this function: dropping it stops advertising.
    let _mdns = match discover::advertise(port) {
        Ok(m) => Some(m),
        Err(e) => {
            eprintln!("mDNS advertising unavailable ({e}), host will need the IP directly.");
            None
        }
    };
    println!("lintas serving on port {port}, waiting for host...");

    for stream in listener.incoming() {
        let tcp = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Accept failed: {e}");
                continue;
            }
        };
        let _ = tcp.set_nodelay(true);
        let peer = tcp.peer_addr().ok();
        // Bounds how long a stalled handshake (accidental or a deliberate
        // hang) can occupy this slot: only one connection is serviced at a
        // time, so without this an unresponsive client would block every
        // legitimate host from ever connecting.
        let _ = tcp.set_read_timeout(Some(Duration::from_secs(10)));
        let conn = match rustls::ServerConnection::new(tls_config.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("TLS setup failed: {e}");
                continue;
            }
        };
        let mut s = tls::ServerStream::new(conn, tcp);
        if let Err(e) = s.conn.complete_io(&mut s.sock) {
            eprintln!("TLS handshake with {peer:?} failed: {e}");
            continue;
        }
        // Pairing is mutual: `host` already verifies *this* machine's
        // identity (see tls.rs). Without also checking the host's identity
        // here, anyone on the network who merely completes a TLS handshake
        // (which needs no secret at all) could inject input into this
        // machine — pairing would only have protected the other direction.
        let host_key = peer.map(|p| p.ip().to_string());
        let pairing = (|| -> io::Result<()> {
            let key = host_key
                .as_deref()
                .ok_or_else(|| io::Error::other("connection has no peer address"))?;
            let fp = tls::peer_fingerprint_server(&s.conn)
                .ok_or_else(|| io::Error::other("host sent no certificate"))?;
            // serve never grabs input devices, so a stdin prompt here is
            // always safe, unlike the equivalent check on the host side.
            tls::confirm_pairing("hosts", key, &fp, true)
        })();
        if let Err(e) = pairing {
            eprintln!("Rejecting connection from {peer:?}: {e}");
            continue;
        }
        let mut hello = Vec::new();
        encode(&mut hello, (T_HELLO, screen.h as u16, screen.w as i32));
        if s.write_all(&hello).is_err() {
            continue;
        }
        println!("Host connected: {peer:?}");
        if let Some(t) = &tray {
            t.set_status(match peer {
                Some(p) => format!("Connected: {p}"),
                None => "Connected".to_string(),
            });
        }
        // Non-blocking, not a read timeout: this loop polls on every real
        // event too (not just idle ticks), so even a short *blocking*
        // timeout would stall input forwarding by that long on every event.
        let _ = s.sock.set_nonblocking(true);

        let mut clip = clipboard::ClipSync::new();
        let mut framer = FrameReader::default();
        let mut pressed: HashSet<u16> = HashSet::new();
        let mut batch: Vec<Ev> = Vec::new();
        loop {
            let buf = match framer.poll(&mut s) {
                Ok(Some(buf)) => buf,
                Ok(None) => {
                    clip.poll_and_send(&mut s)?;
                    // Non-blocking read returned instantly: without this,
                    // idle time turns into a 100%-CPU busy loop. 1ms is way
                    // below anything perceptible as input lag.
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(_) => break,
            };
            let (t, c, v) = decode(&buf);
            if t == T_CLIP {
                clip.receive(&mut s, v.max(0) as usize)?;
                continue;
            }
            if t == T_ENTER {
                // Put the cursor on the entry edge, at the same height it left the host
                let fx = if c == 0 { 1.0 } else { 0.0 };
                placer.place(&mut vdev, fx, v as f64 / RATIO_SCALE)?;
                continue;
            }
            if t == EventType::SYNCHRONIZATION.0 {
                if !batch.is_empty() {
                    vdev.emit(&to_input(&batch))?;
                    batch.clear();
                }
                clip.poll_and_send(&mut s)?;
                continue;
            }
            if t == EventType::KEY.0 {
                if v == 0 {
                    pressed.remove(&c);
                } else {
                    pressed.insert(c);
                }
            }
            batch.push((t, c, v));
        }

        // Host disconnected: release anything still held so no key gets stuck
        let evs: Vec<Ev> = pressed.drain().map(|c| (EventType::KEY.0, c, 0)).collect();
        if !evs.is_empty() {
            vdev.emit(&to_input(&evs))?;
        }
        println!("Host disconnected, waiting again...");
        if let Some(t) = &tray {
            t.set_status("Waiting for host");
        }
    }
    Ok(())
}

// ----------------------------------------------------------------- HOST

fn is_input_device(d: &Device) -> bool {
    if d.name().is_some_and(|n| n.starts_with(VDEV_PREFIX)) {
        return false;
    }
    // Touchpads/touchscreens (ABS) are left alone and keep working on their own machine.
    if d.supported_absolute_axes()
        .is_some_and(|a| a.iter().next().is_some())
    {
        return false;
    }
    let kb = d.supported_keys().is_some_and(|k| k.contains(Key::KEY_A));
    let mouse = d
        .supported_relative_axes()
        .is_some_and(|r| r.contains(RelativeAxisType::REL_X));
    kb || mouse
}

struct Host {
    local: VirtualDevice,
    placer: Placer,
    remote: Option<tls::ClientStream>,
    tls_config: std::sync::Arc<rustls::ClientConfig>,
    peer: SocketAddr,
    side: Side,
    speed: f64,
    host: Screen,
    remote_screen: Screen,
    on_remote: bool,
    /// Estimated cursor position on the host (lx, ly) and on the remote (rx, ry)
    lx: f64,
    ly: f64,
    rx: f64,
    ry: f64,
    /// Physical keys currently held down
    held: HashSet<u16>,
    /// Keys whose press was delivered to the active target
    sent: HashSet<u16>,
    out: Vec<Ev>,
    /// Reused across `send()` calls to avoid allocating a fresh `Vec` on
    /// every flush, on what's otherwise the hottest path in the program
    /// (every mouse-move batch while controlling the remote).
    send_buf: Vec<u8>,
    last_try: Option<Instant>,
    clip: clipboard::ClipSync,
    in_framer: FrameReader,
    tray: Option<tray::TrayHandle>,
}

impl Host {
    /// Reads any pending frames from the remote (currently just clipboard
    /// updates) and pushes local clipboard changes out, without blocking.
    fn service_clipboard(&mut self) -> io::Result<()> {
        if self.remote.is_none() {
            return Ok(());
        }
        let poll_result = self.in_framer.poll(self.remote.as_mut().unwrap());
        match poll_result {
            Ok(Some(buf)) => {
                let (t, _c, v) = decode(&buf);
                if t == T_CLIP {
                    self.clip
                        .receive(self.remote.as_mut().unwrap(), v.max(0) as usize)?;
                }
            }
            Ok(None) => {}
            Err(_) => {
                eprintln!("Lost connection to remote, back to local.");
                self.remote = None;
                self.on_remote = false;
                self.sent.clear();
                self.in_framer = FrameReader::default();
                return Ok(());
            }
        }
        self.clip.poll_and_send(self.remote.as_mut().unwrap())
    }

    /// Deliver buffered events to the active target.
    fn flush(&mut self) -> io::Result<()> {
        if self.out.is_empty() {
            return Ok(());
        }
        let evs = std::mem::take(&mut self.out);
        if self.on_remote {
            if self.send(&evs, true).is_err() {
                eprintln!("Lost connection to remote, back to local.");
                self.remote = None;
                self.on_remote = false;
                self.sent.clear();
            }
        } else {
            self.local.emit(&to_input(&evs))?;
        }
        Ok(())
    }

    fn send(&mut self, evs: &[Ev], syn: bool) -> io::Result<()> {
        let s = self
            .remote
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
        self.send_buf.clear();
        for &e in evs {
            encode(&mut self.send_buf, e);
        }
        if syn {
            encode(&mut self.send_buf, (EventType::SYNCHRONIZATION.0, 0, 0));
        }
        s.write_all(&self.send_buf)
    }

    /// Release every key still held on the active target.
    fn release_sent(&mut self) -> io::Result<()> {
        self.flush()?;
        let rel: Vec<Ev> = self
            .sent
            .drain()
            .map(|c| (EventType::KEY.0, c, 0))
            .collect();
        self.out.extend(rel);
        self.flush()
    }

    /// Connect to the peer, pairing over TLS. `interactive` must only be
    /// true before input devices are grabbed: pairing a new peer can prompt
    /// on stdin, and once devices are grabbed the keyboard driving that
    /// terminal is captured too, so the prompt could never be answered.
    fn connect(&mut self, interactive: bool) -> bool {
        if self.remote.is_some() {
            return true;
        }
        // Don't retry on every single mouse movement
        if self
            .last_try
            .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
        {
            return false;
        }
        self.last_try = Some(Instant::now());
        let res = (|| -> io::Result<(tls::ClientStream, Screen)> {
            let tcp = TcpStream::connect_timeout(&self.peer, Duration::from_millis(500))?;
            tcp.set_nodelay(true)?;
            tcp.set_read_timeout(Some(Duration::from_secs(5)))?;
            let name = rustls::pki_types::ServerName::try_from(self.peer.ip().to_string())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
            let conn = rustls::ClientConnection::new(self.tls_config.clone(), name)
                .map_err(|e| io::Error::other(e.to_string()))?;
            let mut s = tls::ClientStream::new(conn, tcp);
            s.conn.complete_io(&mut s.sock)?;
            let fp = tls::peer_fingerprint(&s.conn).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "peer sent no certificate")
            })?;
            tls::confirm_pairing("peers", &self.peer.to_string(), &fp, interactive)?;
            let mut b = [0u8; 8];
            s.read_exact(&mut b)?;
            let (t, h, w) = decode(&b);
            if t != T_HELLO {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "not a lintas server",
                ));
            }
            // Non-blocking, not a read timeout: `service_clipboard` polls this
            // on every real input event too, not just idle ticks, so even a
            // short *blocking* timeout would stall input forwarding by that
            // much on every single event.
            s.sock.set_nonblocking(true)?;
            Ok((
                s,
                Screen {
                    // Guards against a later `.clamp(min, max)` panicking
                    // (min > max) if a peer ever reports a non-positive size.
                    w: (w as f64).max(1.0),
                    h: (h as f64).max(1.0),
                },
            ))
        })();
        match res {
            Ok((s, screen)) => {
                println!(
                    "Connected to {}, remote screen {}x{} px",
                    self.peer, screen.w, screen.h
                );
                self.remote = Some(s);
                self.remote_screen = screen;
                self.in_framer = FrameReader::default();
                true
            }
            Err(e) => {
                eprintln!("Could not connect to {}: {e}", self.peer);
                false
            }
        }
    }

    fn go_remote(&mut self) -> io::Result<bool> {
        // Devices are grabbed by the time this runs, so never prompt here.
        if self.on_remote || !self.connect(false) {
            return Ok(false);
        }
        self.release_sent()?;
        self.on_remote = true;
        let fy = (self.ly / self.host.h).clamp(0.0, 1.0);
        // Remote on the left: the cursor enters through the remote's right edge, and vice versa
        let (edge, rx) = match self.side {
            Side::Left => (0, self.remote_screen.w),
            Side::Right => (1, 0.0),
        };
        let msg = (T_ENTER, edge, (fy * RATIO_SCALE) as i32);
        if self.send(&[msg], false).is_err() {
            self.remote = None;
            self.on_remote = false;
            return Ok(false);
        }
        self.rx = rx;
        self.ry = fy * self.remote_screen.h;
        println!("-> REMOTE");
        if let Some(t) = &self.tray {
            t.set_status(format!("Controlling {}", self.peer));
        }
        Ok(true)
    }

    fn go_local(&mut self) -> io::Result<()> {
        if !self.on_remote {
            return Ok(());
        }
        self.release_sent()?;
        self.on_remote = false;
        let fy = (self.ry / self.remote_screen.h).clamp(0.0, 1.0);
        // Put the host cursor on the edge facing the remote, at the matching height
        let (fx, lx) = match self.side {
            Side::Left => (0.0, 0.0),
            Side::Right => (1.0, self.host.w),
        };
        self.placer.place(&mut self.local, fx, fy)?;
        self.lx = lx;
        self.ly = fy * self.host.h;
        println!("-> LOCAL");
        if let Some(t) = &self.tray {
            t.set_status("Local");
        }
        Ok(())
    }

    /// Track horizontal motion. Returns true when this event triggered a
    /// switch (the event itself is then dropped).
    fn track_x(&mut self, dx: f64) -> io::Result<bool> {
        let dx = dx * self.speed;
        if !self.on_remote {
            self.lx += dx;
            let crossed = match self.side {
                Side::Left => self.lx < -PUSH,
                Side::Right => self.lx > self.host.w + PUSH,
            };
            if crossed && self.go_remote()? {
                return Ok(true);
            }
            // Clamp at the walls so the estimate resyncs whenever the cursor hits them
            self.lx = match self.side {
                Side::Left => self
                    .lx
                    .clamp(if crossed { 0.0 } else { -PUSH }, self.host.w),
                Side::Right => {
                    let max = if crossed {
                        self.host.w
                    } else {
                        self.host.w + PUSH
                    };
                    self.lx.clamp(0.0, max)
                }
            };
        } else {
            self.rx += dx;
            let back = match self.side {
                Side::Left => self.rx > self.remote_screen.w + PUSH,
                Side::Right => self.rx < -PUSH,
            };
            if back {
                self.go_local()?;
                return Ok(true);
            }
            self.rx = match self.side {
                Side::Left => self.rx.clamp(0.0, self.remote_screen.w + PUSH),
                Side::Right => self.rx.clamp(-PUSH, self.remote_screen.w),
            };
        }
        Ok(false)
    }

    /// Track vertical motion, clamped to the active screen.
    fn track_y(&mut self, dy: f64) {
        let dy = dy * self.speed;
        if self.on_remote {
            self.ry = (self.ry + dy).clamp(0.0, self.remote_screen.h);
        } else {
            self.ly = (self.ly + dy).clamp(0.0, self.host.h);
        }
    }
}

fn hotkey_mods(held: &HashSet<u16>) -> bool {
    let any = |a: Key, b: Key| held.contains(&a.code()) || held.contains(&b.code());
    any(Key::KEY_LEFTCTRL, Key::KEY_RIGHTCTRL)
        && any(Key::KEY_LEFTALT, Key::KEY_RIGHTALT)
        && any(Key::KEY_LEFTSHIFT, Key::KEY_RIGHTSHIFT)
}

fn host(
    peer: &str,
    side: Side,
    screen: Screen,
    speed: f64,
    warp: bool,
    show_tray: bool,
) -> io::Result<()> {
    let tray = show_tray.then(|| tray::spawn("Local")).flatten();
    let peer_addr = peer
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid peer address"))?;
    let local = make_vdev(&format!("{VDEV_PREFIX}-local"))?;
    let placer = Placer::new(&format!("{VDEV_PREFIX}-local-placer"), warp);
    // Presented to `serve` as this machine's identity, so `serve` can pair
    // with (and thereafter recognize) this specific host instead of
    // accepting input from any device that merely completes a handshake.
    let identity = tls::load_or_create_identity()?;
    let tls_config = tls::client_config(&identity)?;

    let mut h = Host {
        local,
        placer,
        remote: None,
        tls_config,
        peer: peer_addr,
        side,
        speed,
        host: screen,
        remote_screen: Screen {
            w: 1920.0,
            h: 1080.0,
        },
        on_remote: false,
        lx: screen.w / 2.0,
        ly: screen.h / 2.0,
        rx: 0.0,
        ry: 0.0,
        held: HashSet::new(),
        sent: HashSet::new(),
        out: Vec::new(),
        send_buf: Vec::new(),
        last_try: None,
        clip: clipboard::ClipSync::new(),
        in_framer: FrameReader::default(),
        tray,
    };
    // Connect (and pair, if needed) before grabbing any input device: pairing
    // can prompt on stdin, and once devices are grabbed the keyboard for this
    // very terminal is captured too, so the prompt could never be answered.
    h.connect(true);

    // Give the user time to release Enter from the terminal before grabbing
    thread::sleep(Duration::from_millis(600));

    let (tx, rx) = mpsc::channel::<Vec<Ev>>();
    let mut count = 0;
    for (path, mut dev) in evdev::enumerate() {
        if !is_input_device(&dev) {
            continue;
        }
        let name = dev.name().unwrap_or("?").to_string();
        if let Err(e) = dev.grab() {
            eprintln!("Could not grab {name} ({}): {e}", path.display());
            continue;
        }
        println!("Capturing: {name}");
        count += 1;
        let tx = tx.clone();
        thread::spawn(move || {
            // Stops when the device is unplugged (fetch_events errors)
            while let Ok(it) = dev.fetch_events() {
                let evs: Vec<Ev> = it
                    .filter(|e| {
                        let t = e.event_type();
                        t == EventType::KEY || t == EventType::RELATIVE
                    })
                    .map(|e| (e.event_type().0, e.code(), e.value()))
                    .collect();
                if !evs.is_empty() && tx.send(evs).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    if count == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "no readable input devices",
        ));
    }

    let dir = if side == Side::Left { "left" } else { "right" };
    println!("Ready. Push the mouse past the {dir} edge to switch to {peer}.");
    println!("Ctrl+Alt+Shift+Space = manual switch, Ctrl+Alt+Shift+Esc = exit.");

    loop {
        let evs = match rx.recv_timeout(POLL_INTERVAL) {
            Ok(evs) => evs,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                h.service_clipboard()?;
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        for (t, c, v) in evs {
            if t == EventType::KEY.0 {
                if v == 0 {
                    h.held.remove(&c);
                } else {
                    h.held.insert(c);
                }
                if v == 1 && hotkey_mods(&h.held) {
                    if c == Key::KEY_ESC.code() {
                        let _ = h.release_sent();
                        println!("Emergency exit.");
                        return Ok(());
                    }
                    if c == Key::KEY_SPACE.code() {
                        if h.on_remote {
                            h.go_local()?;
                        } else {
                            h.go_remote()?;
                        }
                        continue;
                    }
                }
                // Drop key-ups whose press never reached the current target
                if v == 0 && !h.sent.remove(&c) {
                    continue;
                }
                if v == 1 {
                    h.sent.insert(c);
                }
            } else if t == EventType::RELATIVE.0 && c == RelativeAxisType::REL_Y.0 {
                h.track_y(v as f64);
            } else if t == EventType::RELATIVE.0
                && c == RelativeAxisType::REL_X.0
                && h.track_x(v as f64)?
            {
                continue;
            }
            h.out.push((t, c, v));
        }
        h.flush()?;
        h.service_clipboard()?;
    }
    Ok(())
}
