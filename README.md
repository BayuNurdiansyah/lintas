# lintas

> *Lintas* berarti menyeberang dalam bahasa Indonesia. Move your cursor across machines. Zero config, any compositor.

Share mouse dan keyboard antar komputer Linux lewat LAN. Geser mouse ke tepi layar, kursor pindah ke komputer sebelah.

- **Tidak tergantung desktop.** Jalan di X11, Wayland (KDE, GNOME, Hyprland, Sway), bahkan TTY, karena input dibaca dan diinjeksi langsung di level kernel (evdev + uinput).
- **Pindah lewat tepi layar tanpa bantuan compositor.** Lebar semua monitor dideteksi otomatis dari kernel (`/sys/class/drm`).
- **Minim setup.** Satu binary, satu script izin, tanpa file config.

> Status: **alpha**. Koneksi belum terenkripsi, pakai hanya di jaringan yang kamu percaya.

## Instalasi

Butuh Rust (`sudo pacman -S rust` di Arch, atau lewat [rustup](https://rustup.rs) di distro lain).

```bash
git clone https://github.com/<username>/lintas.git
cd lintas
bash packaging/install.sh   # izin akses /dev/input dan /dev/uinput, lalu reboot
cargo build --release
cp target/release/lintas ~/.local/bin/
```

Lakukan di semua komputer.

## Pemakaian

Contoh: laptop di kiri, PC di kanan (PC yang punya mouse dan keyboard).

```bash
# di laptop
lintas serve

# di PC
lintas host <ip-laptop> --side left
```

| Opsi host | Fungsi |
|---|---|
| `--side left\|right` | Posisi komputer remote relatif ke host (default `left`) |
| `--width PX` | Total lebar monitor, kalau deteksi otomatis meleset (misal karena scaling) |
| `--speed F` | Kalibrasi titik pindah, misal `0.8` atau `1.3` |

Hotkey: `Ctrl+Alt+Shift+Space` pindah manual, `Ctrl+Alt+Shift+Esc` keluar darurat.

Port default TCP `4242`. Buka di firewall komputer yang menjalankan `serve`.

Auto-start di sisi serve: lihat `packaging/lintas-serve.service`.

## Cara kerja

1. Host meng-grab semua keyboard dan mouse (evdev), lalu meneruskannya ke virtual device lokal atau ke remote.
2. Posisi horizontal kursor diperkirakan dari gerakan relatif mouse, dan disinkronkan ulang setiap kali kursor mentok di dinding layar.
3. Saat melewati tepi, host mengirim event ke remote. Remote menginjeksinya lewat uinput dan menempelkan kursor ke tepi masuk.

## Roadmap

- [x] Fase 1: forward input evdev ke uinput, pindah dengan hotkey
- [x] Fase 3: pindah lewat tepi layar, deteksi multi monitor
- [ ] Fase 2: auto-discovery (mDNS), pairing kode, enkripsi (TLS/QUIC)
- [ ] Fase 4: UI pengaturan layout + tray icon
- [ ] Sinkron clipboard
- [ ] Capture touchpad di sisi host
- [ ] Lebih dari 2 komputer

## Proyek serupa

- [rkvm](https://github.com/htrefil/rkvm): pendekatan evdev + uinput yang sama, sudah TLS, tapi pindah hanya lewat hotkey dan pakai file config.
- [Lan Mouse](https://github.com/feschber/lan-mouse), [Deskflow](https://github.com/deskflow/deskflow), [Input Leap](https://github.com/input-leap/input-leap): pindah lewat tepi layar, lewat integrasi tiap compositor.

## Lisensi

MIT
