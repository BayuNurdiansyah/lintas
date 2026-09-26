//! Optional config file for defaults, so day-to-day runs (and the systemd
//! units) don't need to repeat the same flags every time.
//!
//! Lives at `~/.config/lintas/config.toml` (or `$XDG_CONFIG_HOME/lintas/...`).
//! Deliberately not a real TOML parser — the format only needs `key = value`
//! pairs, comments, and blank lines, so a tiny hand-rolled parser keeps this
//! dependency-free. Precedence is always: CLI flag > config file > built-in
//! default.

use std::path::PathBuf;

#[derive(Default)]
pub struct Config {
    pub port: Option<u16>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub speed: Option<f64>,
    pub side: Option<String>,
    pub no_warp: Option<bool>,
    /// `host`'s peer, so `lintas host` alone can work without typing an IP
    /// or relying on mDNS discovery.
    pub peer: Option<String>,
}

fn config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config")
        });
    base.join("lintas").join("config.toml")
}

pub fn load() -> Config {
    let Ok(text) = std::fs::read_to_string(config_path()) else {
        return Config::default();
    };
    let mut c = Config::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim().trim_matches('"');
        match key {
            "port" => c.port = val.parse().ok(),
            "width" => c.width = val.parse().ok(),
            "height" => c.height = val.parse().ok(),
            "speed" => c.speed = val.parse().ok(),
            "side" => c.side = Some(val.to_string()),
            "no_warp" => c.no_warp = val.parse().ok(),
            "peer" => c.peer = Some(val.to_string()),
            _ => eprintln!(
                "Ignoring unknown config key '{key}' in {}",
                config_path().display()
            ),
        }
    }
    c
}
