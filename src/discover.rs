//! Zero-config peer discovery over mDNS, so `host` doesn't need a hardcoded IP.
//!
//! `serve` advertises itself as `_lintas._tcp.local.`; `host` can browse for
//! it instead of being given an address. This only replaces "how do I find
//! the IP", not trust: pairing (see [`crate::tls`]) still applies once a
//! peer is picked.

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::time::Duration;

const SERVICE_TYPE: &str = "_lintas._tcp.local.";
/// How long to listen for announcements before giving up or asking the user
/// to pick, respectively.
const BROWSE_TIME: Duration = Duration::from_secs(3);

fn hostname() -> String {
    let name = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        "lintas".to_string()
    } else {
        name
    }
}

/// Advertises this machine as a lintas serve target. Keep the returned
/// daemon alive for as long as `serve` is meant to be discoverable — dropping
/// it stops the background responder thread.
pub fn advertise(port: u16) -> io::Result<ServiceDaemon> {
    let mdns = ServiceDaemon::new().map_err(io::Error::other)?;
    let host = hostname();
    let hostname_local = format!("{host}.local.");
    let no_props: &[(&str, &str)] = &[];
    let info = ServiceInfo::new(SERVICE_TYPE, &host, &hostname_local, "", port, no_props)
        .map_err(io::Error::other)?
        .enable_addr_auto();
    mdns.register(info).map_err(io::Error::other)?;
    println!("Advertising as '{host}' on the local network (mDNS).");
    Ok(mdns)
}

struct Found {
    name: String,
    addr: SocketAddr,
}

/// Browses for lintas serve instances on the LAN and returns the address of
/// the one the user wants to connect to (auto-picked if there's only one).
pub fn find_peer() -> io::Result<SocketAddr> {
    let mdns = ServiceDaemon::new().map_err(io::Error::other)?;
    let receiver = mdns.browse(SERVICE_TYPE).map_err(io::Error::other)?;
    println!("Looking for lintas serve on the local network...");

    let mut found: Vec<Found> = Vec::new();
    let deadline = std::time::Instant::now() + BROWSE_TIME;
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Ok(event) = receiver.recv_timeout(remaining) else {
            break;
        };
        if let ServiceEvent::ServiceResolved(info) = event {
            let Some(ip) = info.get_addresses_v4().into_iter().next() else {
                continue;
            };
            let name = info.get_hostname().trim_end_matches(".local.").to_string();
            let addr = SocketAddr::new(ip.into(), info.get_port());
            if !found.iter().any(|f| f.addr == addr) {
                found.push(Found { name, addr });
            }
        }
    }
    let _ = mdns.shutdown();

    match found.len() {
        0 => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no lintas serve found on the local network (mDNS). Is it running, and is UDP port \
             5353 open? You can also connect by IP directly: lintas host <ip>",
        )),
        1 => {
            println!("Found '{}' at {}.", found[0].name, found[0].addr);
            Ok(found[0].addr)
        }
        _ => {
            println!("Found multiple lintas serve instances:");
            for (i, f) in found.iter().enumerate() {
                println!("  {}) {} ({})", i + 1, f.name, f.addr);
            }
            print!("Which one? [1-{}] ", found.len());
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            let idx: usize = line
                .trim()
                .parse()
                .ok()
                .filter(|&n| n >= 1 && n <= found.len())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid choice"))?;
            Ok(found[idx - 1].addr)
        }
    }
}
