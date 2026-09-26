//! lintas fase 3: share mouse + keyboard antar Linux, pindah lewat tepi layar.
//! Capture pakai evdev, inject pakai uinput. Jalan di X11, Wayland, distro apa pun.
//!
//!   lintas serve [--port N] [--width PX]
//!   lintas host <ip[:port]> [--side left|right] [--width PX] [--speed F]
//!
//! --side  : posisi device remote relatif ke host (default: left)
//! --width : total lebar layar dalam piksel (default: deteksi otomatis semua monitor)
//! --speed : kalibrasi kalau titik pindah terasa terlalu cepat/lambat (default: 1.0)
//!
//! Hotkey di host:
//!   Ctrl+Alt+Shift+Space  pindah manual lokal <-> remote
//!   Ctrl+Alt+Shift+Esc    keluar darurat

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{AttributeSet, Device, EventType, InputEvent, Key, RelativeAxisType};
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_PORT: u16 = 4242;
const VDEV_PREFIX: &str = "lintas";
/// Seberapa jauh mouse harus "didorong" melewati tepi sebelum pindah.
const PUSH: f64 = 25.0;
/// Gerakan besar untuk menempelkan kursor ke tepi layar (dijepit oleh compositor).
const SLAM: i32 = 100_000;

// Pesan kontrol khusus, di luar range tipe event evdev
const T_HELLO: u16 = 0xFFF0; // serve -> host, value = lebar layar
const T_ENTER: u16 = 0xFFF1; // host -> serve, value = 0 masuk dari kanan, 1 dari kiri

type Ev = (u16, u16, i32);

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let res = match args.get(1).map(|s| s.as_str()) {
        Some("serve") => {
            let port = opt(&args, "--port")
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            let width = opt(&args, "--width")
                .and_then(|w| w.parse().ok())
                .unwrap_or_else(detect_width);
            serve(port, width)
        }
        Some("host") if args.len() > 2 && !args[2].starts_with("--") => {
            let mut peer = args[2].clone();
            if !peer.contains(':') {
                peer = format!("{peer}:{DEFAULT_PORT}");
            }
            let side = match opt(&args, "--side").as_deref() {
                Some("right") => Side::Right,
                _ => Side::Left,
            };
            let width = opt(&args, "--width")
                .and_then(|w| w.parse().ok())
                .unwrap_or_else(detect_width);
            let speed = opt(&args, "--speed")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1.0);
            host(&peer, side, width, speed)
        }
        _ => {
            eprintln!(
                "Pakai:\n  lintas serve [--port N] [--width PX]\n  \
                 lintas host <ip[:port]> [--side left|right] [--width PX] [--speed F]"
            );
            std::process::exit(1);
        }
    };
    if let Err(e) = res {
        eprintln!("Error: {e}");
        if e.kind() == io::ErrorKind::PermissionDenied {
            eprintln!("Jalankan packaging/install.sh lalu login ulang.");
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

/// Jumlahkan lebar semua monitor yang terhubung, dibaca dari kernel (DRM).
/// Tidak tergantung X11/Wayland/compositor.
fn detect_width() -> u32 {
    let mut total = 0;
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
            if let Some(w) = read("modes")
                .lines()
                .next()
                .and_then(|m| m.split('x').next())
                .and_then(|w| w.parse::<u32>().ok())
            {
                println!("Monitor {name}: lebar {w}px");
                total += w;
            }
        }
    }
    if total == 0 {
        println!("Monitor tidak terdeteksi, pakai 1920px (atur dengan --width)");
        1920
    } else {
        total
    }
}

/// Virtual device dengan semua tombol + sumbu relatif mouse.
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

fn to_input(evs: &[Ev]) -> Vec<InputEvent> {
    evs.iter()
        .map(|&(t, c, v)| InputEvent::new(EventType(t), c, v))
        .collect()
}

fn encode(buf: &mut Vec<u8>, (t, c, v): Ev) {
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

fn slam_x(vdev: &mut VirtualDevice, dir: i32) -> io::Result<()> {
    vdev.emit(&to_input(&[(
        EventType::RELATIVE.0,
        RelativeAxisType::REL_X.0,
        dir * SLAM,
    )]))
}

// ---------------------------------------------------------------- SERVE

fn serve(port: u16, width: u32) -> io::Result<()> {
    let mut vdev = make_vdev(&format!("{VDEV_PREFIX}-remote"))?;
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    println!("lintas serve di port {port}, lebar layar {width}px, menunggu host...");
    for stream in listener.incoming() {
        let mut s = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("accept gagal: {e}");
                continue;
            }
        };
        let _ = s.set_nodelay(true);
        let mut hello = Vec::new();
        encode(&mut hello, (T_HELLO, 0, width as i32));
        if s.write_all(&hello).is_err() {
            continue;
        }
        println!("Host terhubung: {:?}", s.peer_addr().ok());

        let mut pressed: HashSet<u16> = HashSet::new();
        let mut batch: Vec<Ev> = Vec::new();
        let mut buf = [0u8; 8];
        while s.read_exact(&mut buf).is_ok() {
            let (t, c, v) = decode(&buf);
            if t == T_ENTER {
                // Tempelkan kursor ke tepi tempat mouse masuk
                slam_x(&mut vdev, if v == 0 { 1 } else { -1 })?;
                continue;
            }
            if t == EventType::SYNCHRONIZATION.0 {
                if !batch.is_empty() {
                    vdev.emit(&to_input(&batch))?;
                    batch.clear();
                }
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
        let evs: Vec<Ev> = pressed.drain().map(|c| (EventType::KEY.0, c, 0)).collect();
        if !evs.is_empty() {
            vdev.emit(&to_input(&evs))?;
        }
        println!("Host terputus, menunggu lagi...");
    }
    Ok(())
}

// ----------------------------------------------------------------- HOST

fn is_input_device(d: &Device) -> bool {
    if d.name().is_some_and(|n| n.starts_with(VDEV_PREFIX)) {
        return false;
    }
    // Touchpad/touchscreen (ABS) tidak di-grab: tetap dipakai normal di device-nya sendiri.
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
    remote: Option<TcpStream>,
    peer: SocketAddr,
    side: Side,
    speed: f64,
    host_w: f64,
    remote_w: f64,
    on_remote: bool,
    /// Posisi horizontal kursor (perkiraan) di host dan di remote
    lx: f64,
    rx: f64,
    held: HashSet<u16>,
    sent: HashSet<u16>,
    out: Vec<Ev>,
    last_try: Option<Instant>,
}

impl Host {
    fn flush(&mut self) -> io::Result<()> {
        if self.out.is_empty() {
            return Ok(());
        }
        let evs = std::mem::take(&mut self.out);
        if self.on_remote {
            if self.send(&evs, true).is_err() {
                eprintln!("Koneksi ke remote putus, kembali ke lokal.");
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
        let mut buf = Vec::with_capacity((evs.len() + 1) * 8);
        for &e in evs {
            encode(&mut buf, e);
        }
        if syn {
            encode(&mut buf, (EventType::SYNCHRONIZATION.0, 0, 0));
        }
        s.write_all(&buf)
    }

    /// Lepas semua tombol yang masih ditekan di target aktif.
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

    fn connect(&mut self) -> bool {
        if self.remote.is_some() {
            return true;
        }
        // Jangan coba konek terus-terusan tiap gerakan mouse
        if self
            .last_try
            .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
        {
            return false;
        }
        self.last_try = Some(Instant::now());
        let res = (|| -> io::Result<(TcpStream, f64)> {
            let mut s = TcpStream::connect_timeout(&self.peer, Duration::from_millis(500))?;
            s.set_nodelay(true)?;
            s.set_read_timeout(Some(Duration::from_secs(1)))?;
            let mut b = [0u8; 8];
            s.read_exact(&mut b)?;
            let (t, _, w) = decode(&b);
            if t != T_HELLO {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "bukan server lintas",
                ));
            }
            s.set_read_timeout(None)?;
            Ok((s, w as f64))
        })();
        match res {
            Ok((s, w)) => {
                println!("Terhubung ke {}, lebar layar remote {w}px", self.peer);
                self.remote = Some(s);
                self.remote_w = w;
                true
            }
            Err(e) => {
                eprintln!("Gagal konek ke {}: {e}", self.peer);
                false
            }
        }
    }

    fn go_remote(&mut self) -> io::Result<bool> {
        if self.on_remote || !self.connect() {
            return Ok(false);
        }
        self.release_sent()?;
        self.on_remote = true;
        // Remote di kiri: kursor masuk dari tepi kanan layar remote, dan sebaliknya
        let (enter, rx) = match self.side {
            Side::Left => (0, self.remote_w),
            Side::Right => (1, 0.0),
        };
        if self.send(&[(T_ENTER, 0, enter)], false).is_err() {
            self.remote = None;
            self.on_remote = false;
            return Ok(false);
        }
        self.rx = rx;
        println!("-> REMOTE");
        Ok(true)
    }

    fn go_local(&mut self) -> io::Result<()> {
        if !self.on_remote {
            return Ok(());
        }
        self.release_sent()?;
        self.on_remote = false;
        // Tempelkan kursor host ke tepi yang berbatasan dengan remote
        let (dir, lx) = match self.side {
            Side::Left => (-1, 0.0),
            Side::Right => (1, self.host_w),
        };
        slam_x(&mut self.local, dir)?;
        self.lx = lx;
        println!("-> LOKAL");
        Ok(())
    }

    /// Update posisi horizontal. Return true kalau event ini memicu perpindahan
    /// (event-nya tidak diteruskan).
    fn track_x(&mut self, dx: f64) -> io::Result<bool> {
        let dx = dx * self.speed;
        if !self.on_remote {
            self.lx += dx;
            let crossed = match self.side {
                Side::Left => self.lx < -PUSH,
                Side::Right => self.lx > self.host_w + PUSH,
            };
            if crossed && self.go_remote()? {
                return Ok(true);
            }
            // Dijepit di dinding supaya perkiraan posisi otomatis sinkron lagi
            self.lx = match self.side {
                Side::Left => self
                    .lx
                    .clamp(if crossed { 0.0 } else { -PUSH }, self.host_w),
                Side::Right => self.lx.clamp(
                    0.0,
                    if crossed {
                        self.host_w
                    } else {
                        self.host_w + PUSH
                    },
                ),
            };
        } else {
            self.rx += dx;
            let back = match self.side {
                Side::Left => self.rx > self.remote_w + PUSH,
                Side::Right => self.rx < -PUSH,
            };
            if back {
                self.go_local()?;
                return Ok(true);
            }
            self.rx = self.rx.clamp(-PUSH, self.remote_w + PUSH);
            self.rx = match self.side {
                Side::Left => self.rx.max(0.0),
                Side::Right => self.rx.min(self.remote_w),
            };
        }
        Ok(false)
    }
}

fn hotkey_mods(held: &HashSet<u16>) -> bool {
    let any = |a: Key, b: Key| held.contains(&a.code()) || held.contains(&b.code());
    any(Key::KEY_LEFTCTRL, Key::KEY_RIGHTCTRL)
        && any(Key::KEY_LEFTALT, Key::KEY_RIGHTALT)
        && any(Key::KEY_LEFTSHIFT, Key::KEY_RIGHTSHIFT)
}

fn host(peer: &str, side: Side, width: u32, speed: f64) -> io::Result<()> {
    let peer_addr = peer
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "alamat peer tidak valid"))?;
    let local = make_vdev(&format!("{VDEV_PREFIX}-local"))?;
    // Tunggu sebentar supaya tombol Enter dari terminal sudah dilepas sebelum grab
    thread::sleep(Duration::from_millis(600));

    let (tx, rx) = mpsc::channel::<Vec<Ev>>();
    let mut count = 0;
    for (path, mut dev) in evdev::enumerate() {
        if !is_input_device(&dev) {
            continue;
        }
        let name = dev.name().unwrap_or("?").to_string();
        if let Err(e) = dev.grab() {
            eprintln!("Gagal grab {name} ({}): {e}", path.display());
            continue;
        }
        println!("Capture: {name}");
        count += 1;
        let tx = tx.clone();
        thread::spawn(move || {
            // Berhenti kalau device dicabut (fetch_events error)
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
            "tidak ada device input yang bisa dibaca",
        ));
    }

    let mut h = Host {
        local,
        remote: None,
        peer: peer_addr,
        side,
        speed,
        host_w: width as f64,
        remote_w: 1920.0,
        on_remote: false,
        lx: width as f64 / 2.0,
        rx: 0.0,
        held: HashSet::new(),
        sent: HashSet::new(),
        out: Vec::new(),
        last_try: None,
    };
    h.connect(); // coba konek di awal, kalau gagal dicoba lagi saat mouse ke tepi

    let arah = if side == Side::Left { "kiri" } else { "kanan" };
    println!("Siap. Geser mouse ke tepi {arah} layar untuk pindah ke {peer}.");
    println!("Ctrl+Alt+Shift+Space = pindah manual, Ctrl+Alt+Shift+Esc = keluar.");

    for evs in rx {
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
                        println!("Keluar darurat.");
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
                // key-up yang press-nya tidak pernah dikirim ke target ini: buang
                if v == 0 && !h.sent.remove(&c) {
                    continue;
                }
                if v == 1 {
                    h.sent.insert(c);
                }
            } else if t == EventType::RELATIVE.0
                && c == RelativeAxisType::REL_X.0
                && h.track_x(v as f64)?
            {
                continue;
            }
            h.out.push((t, c, v));
        }
        h.flush()?;
    }
    Ok(())
}
