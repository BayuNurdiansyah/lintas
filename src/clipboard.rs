//! Clipboard sync between the two machines, piggybacked on the same TLS
//! connection as input events.
//!
//! Wire shape: a normal 8-byte frame with type [`crate::T_CLIP`] whose
//! `value` is the byte length of the UTF-8 text that immediately follows on
//! the wire (not itself framed as 8-byte chunks, since clipboard contents
//! can be arbitrarily long).

use crate::{encode, T_CLIP};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

/// Don't bother checking the system clipboard more often than this.
const CHECK_EVERY: Duration = Duration::from_millis(500);
/// Refuse to sync anything larger than this, so a huge clipboard (e.g. a
/// copied file's worth of text) can't stall the connection.
const MAX_LEN: usize = 1 << 20;

pub struct ClipSync {
    clipboard: Option<arboard::Clipboard>,
    /// The last text we either sent or received, so we don't echo it back
    /// and forth forever.
    last: Option<String>,
    last_check: Instant,
}

impl ClipSync {
    pub fn new() -> Self {
        let clipboard = arboard::Clipboard::new()
            .inspect_err(|e| eprintln!("Clipboard sync unavailable: {e}"))
            .ok();
        Self {
            clipboard,
            last: None,
            last_check: Instant::now() - CHECK_EVERY,
        }
    }

    /// Checks the local clipboard (at most every [`CHECK_EVERY`]) and, if it
    /// changed, sends it to `out`.
    pub fn poll_and_send(&mut self, out: &mut impl Write) -> io::Result<()> {
        let Some(cb) = self.clipboard.as_mut() else {
            return Ok(());
        };
        if self.last_check.elapsed() < CHECK_EVERY {
            return Ok(());
        }
        self.last_check = Instant::now();
        let Ok(text) = cb.get_text() else {
            return Ok(());
        };
        if text.is_empty() || text.len() > MAX_LEN || self.last.as_deref() == Some(text.as_str()) {
            return Ok(());
        }
        self.last = Some(text.clone());
        let mut buf = Vec::with_capacity(8 + text.len());
        encode(&mut buf, (T_CLIP, 0, text.len() as i32));
        buf.extend_from_slice(text.as_bytes());
        out.write_all(&buf)
    }

    /// Reads a clipboard payload of `len` bytes that follows a [`T_CLIP`]
    /// frame already consumed by the caller, and applies it locally.
    pub fn receive(&mut self, stream: &mut impl Read, len: usize) -> io::Result<()> {
        if len > MAX_LEN {
            // Don't silently read only the first MAX_LEN bytes: the peer
            // already committed to sending `len` bytes, so anything less
            // leaves the rest sitting on the stream and desyncs every frame
            // read after this one. Treat an oversized claim as a protocol
            // violation and drop the connection instead.
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("clipboard payload too large ({len} bytes, max {MAX_LEN})"),
            ));
        }
        let mut bytes = vec![0u8; len];
        read_exact_patient(stream, &mut bytes)?;
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(());
        };
        self.last = Some(text.clone());
        if let Some(cb) = self.clipboard.as_mut() {
            let _ = cb.set_text(text);
        }
        Ok(())
    }
}

/// Give up on a stalled payload after this long, rather than hanging the
/// connection (and, on `serve`, blocking every other connection behind it
/// since only one is serviced at a time) forever.
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Like `read_exact`, but treats a read timeout as "keep waiting" instead
/// of aborting and losing the bytes already read — the stream is
/// non-blocking, so the caller can poll for other work in between. Gives up
/// after [`RECEIVE_TIMEOUT`] if the peer never finishes sending.
fn read_exact_patient(stream: &mut impl Read, buf: &mut [u8]) -> io::Result<()> {
    let deadline = Instant::now() + RECEIVE_TIMEOUT;
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed",
                ))
            }
            Ok(n) => filled += n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "clipboard payload took too long to arrive",
                    ));
                }
                // The header already promised this many bytes are coming, so
                // a short sleep here just avoids busy-spinning a CPU core
                // while they arrive, without meaningfully adding latency.
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
