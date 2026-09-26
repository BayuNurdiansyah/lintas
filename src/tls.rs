//! Transport encryption and trust-on-first-use (TOFU) pairing.
//!
//! Every machine generates its own self-signed certificate on first run and
//! keeps it under `~/.local/share/lintas/`. There is no certificate
//! authority: instead, on the first connection to a given peer, both sides
//! print a 6-digit pairing code derived from the serve side's certificate.
//! If the codes match on both screens, the host accepts and remembers the
//! serve's certificate fingerprint; any later connection with a different
//! fingerprint (e.g. a spoofed peer) is refused instead of silently
//! re-pairing.

use ring::digest;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, ServerConfig, SignatureScheme,
};
use std::io::{self, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;

pub type ServerStream = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;
pub type ClientStream = rustls::StreamOwned<ClientConnection, TcpStream>;

fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share")
        });
    base.join("lintas")
}

/// This machine's persistent self-signed identity (generated once, then reused
/// so its fingerprint, and therefore its pairing code, stays stable).
pub struct Identity {
    pub cert: CertificateDer<'static>,
    key_der: Vec<u8>,
}

impl Identity {
    pub fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()))
    }
}

pub fn load_or_create_identity() -> io::Result<Identity> {
    let dir = state_dir();
    std::fs::create_dir_all(&dir)?;
    let cert_path = dir.join("cert.der");
    let key_path = dir.join("key.der");
    if let (Ok(cert), Ok(key_der)) = (std::fs::read(&cert_path), std::fs::read(&key_path)) {
        return Ok(Identity {
            cert: CertificateDer::from(cert),
            key_der,
        });
    }
    let identity = generate_identity()?;
    std::fs::write(&cert_path, identity.cert.as_ref())?;
    std::fs::write(&key_path, &identity.key_der)?;
    Ok(identity)
}

fn generate_identity() -> io::Result<Identity> {
    let keypair = rcgen::KeyPair::generate().map_err(|e| io::Error::other(e.to_string()))?;
    let params = rcgen::CertificateParams::new(vec!["lintas".to_string()])
        .map_err(|e| io::Error::other(e.to_string()))?;
    let cert = params
        .self_signed(&keypair)
        .map_err(|e| io::Error::other(e.to_string()))?;
    let cert_der = cert.der().to_vec();
    let key_der = keypair.serialize_der();
    Ok(Identity {
        cert: CertificateDer::from(cert_der),
        key_der,
    })
}

/// SHA-256 fingerprint of a certificate, as used for TOFU pinning.
pub fn fingerprint(cert: &CertificateDer<'_>) -> [u8; 32] {
    digest::digest(&digest::SHA256, cert.as_ref())
        .as_ref()
        .try_into()
        .expect("SHA-256 digest is always 32 bytes")
}

/// A memorable 6-digit code derived from a fingerprint, for the human to
/// compare between the two machines' screens.
pub fn pairing_code(fp: &[u8; 32]) -> u32 {
    let n = u32::from_be_bytes([fp[0], fp[1], fp[2], fp[3]]);
    n % 1_000_000
}

fn crypto_provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

pub fn server_config(id: &Identity) -> io::Result<Arc<ServerConfig>> {
    let cfg = ServerConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| io::Error::other(e.to_string()))?
        .with_no_client_auth()
        .with_single_cert(vec![id.cert.clone()], id.key())
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(Arc::new(cfg))
}

/// Accepts any server certificate: there is no CA, trust is established out
/// of band via [`pairing_code`]. The handshake signature is still verified
/// cryptographically, so a connection can't be hijacked mid-session; what is
/// skipped is only the "is this cert issued by someone I trust" CA check.
#[derive(Debug)]
struct AcceptAnyServerCert(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

pub fn client_config() -> io::Result<Arc<ClientConfig>> {
    let provider = crypto_provider();
    let cfg = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| io::Error::other(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert(provider)))
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// Peer's certificate fingerprint, once the handshake has completed.
pub fn peer_fingerprint(conn: &ClientConnection) -> Option<[u8; 32]> {
    conn.peer_certificates()
        .and_then(|certs| certs.first())
        .map(fingerprint)
}

// -------------------------------------------------------------- Trust store

/// Where we remember which fingerprint we've already paired with for a given
/// peer address, so re-connecting doesn't ask for confirmation every time.
fn trust_path() -> PathBuf {
    state_dir().join("trusted_peers")
}

fn load_trust() -> Vec<(String, String)> {
    let Ok(s) = std::fs::read_to_string(trust_path()) else {
        return Vec::new();
    };
    s.lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn save_trust(entries: &[(String, String)]) -> io::Result<()> {
    let dir = state_dir();
    std::fs::create_dir_all(&dir)?;
    let body: String = entries.iter().map(|(a, b)| format!("{a} {b}\n")).collect();
    std::fs::write(trust_path(), body)
}

/// Outcome of checking a peer's fingerprint against what's on file.
pub enum Trust {
    /// Matches what we already trusted for this peer.
    Known,
    /// First time seeing this peer; caller should show the pairing code and
    /// ask the user to confirm before calling [`remember`].
    New,
    /// A *different* fingerprint was trusted before for this peer address.
    /// Likely a reinstalled machine, or someone else answering on that IP.
    Changed,
}

pub fn check(peer: &str, fp: &[u8; 32]) -> Trust {
    let hex = hex_encode(fp);
    match load_trust().into_iter().find(|(a, _)| a == peer) {
        Some((_, known)) if known == hex => Trust::Known,
        Some(_) => Trust::Changed,
        None => Trust::New,
    }
}

pub fn remember(peer: &str, fp: &[u8; 32]) -> io::Result<()> {
    let mut entries = load_trust();
    entries.retain(|(a, _)| a != peer);
    entries.push((peer.to_string(), hex_encode(fp)));
    save_trust(&entries)
}

/// Checks a just-connected peer's certificate against the trust store,
/// pairing interactively on first contact. Returns an error (refusing the
/// connection) if the user declines, or if the peer's fingerprint changed
/// since it was last trusted.
pub fn confirm_pairing(peer: &str, fp: &[u8; 32]) -> io::Result<()> {
    match check(peer, fp) {
        Trust::Known => Ok(()),
        Trust::Changed => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{peer} presented a DIFFERENT certificate than the one we paired with before. \
                 Refusing to connect (this could mean the machine was reinstalled, or someone \
                 else is answering on that address). If you're sure it's expected, remove its \
                 entry from {} and reconnect to re-pair.",
                trust_path().display()
            ),
        )),
        Trust::New => {
            println!(
                "First time connecting to {peer}. Pairing code: {:06}",
                pairing_code(fp)
            );
            print!("Does this match the code shown on {peer}'s screen? [y/N] ");
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            if line.trim().eq_ignore_ascii_case("y") {
                remember(peer, fp)?;
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "pairing declined by user",
                ))
            }
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn pairing_code_is_stable_and_in_range() {
        let fp = [7u8; 32];
        let a = pairing_code(&fp);
        let b = pairing_code(&fp);
        assert_eq!(a, b);
        assert!(a < 1_000_000);
    }

    #[test]
    fn different_certs_fingerprint_differently() {
        let a = generate_identity().unwrap();
        let b = generate_identity().unwrap();
        assert_ne!(fingerprint(&a.cert), fingerprint(&b.cert));
    }

    /// End-to-end TLS handshake over a real loopback socket: the client
    /// should see exactly the server's certificate fingerprint, and
    /// encrypted bytes should actually round-trip.
    #[test]
    fn loopback_handshake_and_fingerprint_match() {
        let server_id = generate_identity().unwrap();
        let server_fp = fingerprint(&server_id.cert);
        let server_cfg = server_config(&server_id).unwrap();
        let client_cfg = client_config().unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let conn = rustls::ServerConnection::new(server_cfg).unwrap();
            let mut s = ServerStream::new(conn, tcp);
            let mut buf = [0u8; 5];
            s.read_exact(&mut buf).unwrap();
            assert_eq!(&buf, b"hello");
        });

        let tcp = TcpStream::connect(addr).unwrap();
        let name = ServerName::try_from("127.0.0.1").unwrap();
        let conn = rustls::ClientConnection::new(client_cfg, name).unwrap();
        let mut s = ClientStream::new(conn, tcp);
        s.conn.complete_io(&mut s.sock).unwrap();
        let fp = peer_fingerprint(&s.conn).expect("server presented a certificate");
        assert_eq!(fp, server_fp);
        s.write_all(b"hello").unwrap();

        server.join().unwrap();
    }
}
