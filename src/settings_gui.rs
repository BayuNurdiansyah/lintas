//! `lintas settings`: a small native window (Slint) for editing
//! `~/.config/lintas/config.toml` without hand-writing it. Every field here
//! maps 1:1 to a key the CLI already reads (see [`crate::config`]) — this is
//! purely a friendlier editor for that same file, not a separate config
//! system or a launcher.

use crate::config;
use std::io;

slint::slint! {
    import { LineEdit, CheckBox, ComboBox, Button, VerticalBox, HorizontalBox } from "std-widgets.slint";

    export component SettingsWindow inherits Window {
        title: "lintas settings";
        preferred-width: 380px;
        preferred-height: 480px;

        in-out property <string> peer;
        in-out property <string> side: "left";
        in-out property <string> port;
        in-out property <string> width_px;
        in-out property <string> height_px;
        in-out property <string> speed;
        in-out property <bool> no_warp;
        in-out property <bool> tray;
        in-out property <string> saved_message;

        callback save();

        VerticalBox {
            Text {
                text: "Leave a field empty to fall back to auto-detection / the built-in default.";
                wrap: word-wrap;
                font-size: 12px;
            }
            Text { text: "Peer (host only, e.g. 192.168.1.50:4242)"; }
            LineEdit { text <=> peer; }
            Text { text: "Side (host only)"; }
            ComboBox { model: ["left", "right"]; current-value <=> side; }
            Text { text: "Port"; }
            LineEdit { text <=> port; input-type: number; }
            HorizontalBox {
                VerticalBox {
                    Text { text: "Width (px)"; }
                    LineEdit { text <=> width_px; input-type: number; }
                }
                VerticalBox {
                    Text { text: "Height (px)"; }
                    LineEdit { text <=> height_px; input-type: number; }
                }
            }
            Text { text: "Speed (host only, e.g. 1.0)"; }
            LineEdit { text <=> speed; }
            CheckBox { text: "Disable exact cursor placement (--no-warp)"; checked <=> no_warp; }
            CheckBox { text: "Show tray icon (--tray)"; checked <=> tray; }
            Button { text: "Save"; clicked => { save(); } }
            Text { text: saved_message; color: #2e7d32; }
        }
    }
}

fn opt_string(f: Option<impl ToString>) -> slint::SharedString {
    f.map(|v| v.to_string()).unwrap_or_default().into()
}

fn parsed<T: std::str::FromStr>(s: &str) -> Option<T> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        s.parse().ok()
    }
}

pub fn run() -> io::Result<()> {
    let cfg = config::load();
    let window = SettingsWindow::new().map_err(io::Error::other)?;

    window.set_peer(cfg.peer.clone().unwrap_or_default().into());
    window.set_side(
        cfg.side
            .clone()
            .unwrap_or_else(|| "left".to_string())
            .into(),
    );
    window.set_port(opt_string(cfg.port));
    window.set_width_px(opt_string(cfg.width));
    window.set_height_px(opt_string(cfg.height));
    window.set_speed(opt_string(cfg.speed));
    window.set_no_warp(cfg.no_warp.unwrap_or(false));
    window.set_tray(cfg.tray.unwrap_or(false));

    let weak = window.as_weak();
    window.on_save(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let new_cfg = config::Config {
            port: parsed(&window.get_port()),
            width: parsed(&window.get_width_px()),
            height: parsed(&window.get_height_px()),
            speed: parsed(&window.get_speed()),
            side: {
                let s = window.get_side().to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            },
            no_warp: Some(window.get_no_warp()),
            tray: Some(window.get_tray()),
            peer: {
                let p = window.get_peer().to_string();
                if p.trim().is_empty() {
                    None
                } else {
                    Some(p)
                }
            },
        };
        match config::save(&new_cfg) {
            Ok(()) => window.set_saved_message("Saved.".into()),
            Err(e) => window.set_saved_message(format!("Could not save: {e}").into()),
        }
    });

    window.run().map_err(io::Error::other)
}
