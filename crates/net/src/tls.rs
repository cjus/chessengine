//! The encrypted link: TLS 1.3 over a TCP stream, with each side recognising the other by the
//! fingerprint of its self-signed certificate.
//!
//! ## What is checked, and where
//!
//! TLS proves that the peer holds the private key of the certificate it sent: that is the part
//! rustls checks here, in both directions (a signature over the handshake, so a client can't
//! claim a fingerprint it doesn't own). What the certificate *means* is not a chain to an
//! authority but the pinned fingerprint, which this crate checks after the handshake: the
//! server's by the client (`crate::client`), the client's by the listener's approved list
//! (`crate::trust`). So the verifiers below accept any self-signed certificate whose signature
//! is sound, and say nothing about whether it is welcome.
//!
//! ## Reading and writing at once
//!
//! The CNP node reads requests on one thread and writes replies from another, while a search
//! runs, and a rustls connection is one object holding both directions. The connection sits
//! behind a mutex that is **never held across a blocking read of the socket**: the reader waits
//! for bytes with the lock free, then takes the lock only to hand them over, so a search can
//! write its replies while the reader waits for the next request. (A single lock held through the
//! read would stall every reply until the client spoke again.)

use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    ClientConfig, DigitallySignedStruct, DistinguishedName, Error, ServerConfig, ServerConnection,
    SignatureScheme,
};

use crate::deadline::Gate;
use crate::identity::{Fingerprint, Identity};

/// Bytes read from the socket at a time.
const CHUNK: usize = 16 * 1024;

/// The crypto provider: `ring`, which compiles on every release target.
pub(crate) fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A server's configuration: TLS 1.3 only, this identity, and a client certificate required
/// (whose fingerprint the listener then checks).
pub(crate) fn server_config(identity: &Identity) -> io::Result<Arc<ServerConfig>> {
    let provider = provider();
    let verifier = Arc::new(AnyClient {
        algorithms: provider.signature_verification_algorithms,
    });
    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![identity.certificate()], identity.key().into())
        .map_err(io::Error::other)?;
    Ok(Arc::new(config))
}

/// A client's configuration: TLS 1.3 only, this identity, and the server's certificate accepted
/// only if its fingerprint is `pin` (or any, if `None`, for a client that is learning it).
pub(crate) fn client_config(
    identity: &Identity,
    pin: Option<Fingerprint>,
) -> io::Result<Arc<ClientConfig>> {
    let provider = provider();
    let verifier = Arc::new(PinnedServer {
        pin,
        algorithms: provider.signature_verification_algorithms,
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(vec![identity.certificate()], identity.key().into())
        .map_err(io::Error::other)?;
    Ok(Arc::new(config))
}

/// Accepts any client certificate whose handshake signature checks out. Whether the client may
/// connect is decided afterwards, by its fingerprint.
#[derive(Debug)]
struct AnyClient {
    algorithms: WebPkiSupportedAlgorithms,
}

impl ClientCertVerifier for AnyClient {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        // No authority to name: the client sends its certificate whatever the hint.
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        // The proof that the client holds the key of the certificate it sent. Skipping it would
        // let anyone present an approved client's certificate, which is public.
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// Accepts the server certificate whose fingerprint is the pinned one.
#[derive(Debug)]
struct PinnedServer {
    pin: Option<Fingerprint>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedServer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self.pin {
            Some(pin) if Fingerprint::of(end_entity) != pin => Err(Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            )),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// The server's side of a TLS connection, shared by its reader and its writer.
type Shared = Arc<Mutex<ServerConnection>>;

fn lock(connection: &Shared) -> MutexGuard<'_, ServerConnection> {
    // A panic elsewhere must not stop the connection from being closed.
    connection.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sends whatever TLS has ready for the peer: handshake messages, alerts, encrypted replies.
fn send_pending(connection: &mut ServerConnection, mut socket: &TcpStream) -> io::Result<()> {
    while connection.wants_write() {
        connection.write_tls(&mut socket)?;
    }
    Ok(())
}

/// Completes the handshake with a client that just connected, and returns what the node reads
/// and writes through, with the fingerprint of the certificate the client proved it holds. The
/// caller decides whether that fingerprint is welcome. The `gate` limits the whole handshake
/// by the connection's deadline.
pub(crate) fn accept(
    gate: Gate,
    stream: TcpStream,
    config: Arc<ServerConfig>,
) -> io::Result<(TlsReader, TlsWriter, Fingerprint)> {
    let connection: Shared = Arc::new(Mutex::new(
        ServerConnection::new(config).map_err(io::Error::other)?,
    ));
    let mut reader = TlsReader {
        connection: Arc::clone(&connection),
        gate,
        socket: stream.try_clone()?,
        buffer: Vec::new(),
        position: 0,
    };
    while lock(&connection).is_handshaking() {
        if reader.pump()? {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
    }
    let fingerprint = lock(&connection)
        .peer_certificates()
        .and_then(|chain| chain.first())
        .map(Fingerprint::of)
        .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "no certificate"))?;
    let writer = TlsWriter {
        connection,
        socket: stream,
    };
    Ok((reader, writer, fingerprint))
}

/// What a node reads requests from: the decrypted bytes of a TLS connection.
pub(crate) struct TlsReader {
    connection: Shared,
    /// Where bytes come from, limited by the connection's deadline until its first message.
    gate: Gate,
    /// The same socket, to send what TLS answers with (alerts, key updates).
    socket: TcpStream,
    /// Decrypted bytes not yet consumed, from `position`.
    buffer: Vec<u8>,
    position: usize,
}

impl TlsReader {
    /// Waits for bytes from the socket (the lock is free meanwhile) and hands them to TLS. True
    /// when the peer has closed its side.
    fn pump(&mut self) -> io::Result<bool> {
        let mut raw = [0u8; CHUNK];
        let read = self.gate.read(&mut raw)?;
        if read == 0 {
            return Ok(true);
        }
        let mut connection = lock(&self.connection);
        let mut fed = &raw[..read];
        while !fed.is_empty() {
            if connection.read_tls(&mut fed)? == 0 {
                return Err(io::Error::other(
                    "TLS would not take the bytes it was given",
                ));
            }
            if let Err(e) = connection.process_new_packets() {
                // An alert tells the peer why before the connection ends.
                let _ = send_pending(&mut connection, &self.socket);
                return Err(io::Error::new(io::ErrorKind::InvalidData, e));
            }
        }
        send_pending(&mut connection, &self.socket)?;
        Ok(false)
    }

    /// Refills the buffer with decrypted bytes, waiting for the peer if there are none; leaves it
    /// empty when the peer has closed.
    fn refill(&mut self) -> io::Result<()> {
        self.buffer.clear();
        self.position = 0;
        loop {
            let mut chunk = [0u8; CHUNK];
            match lock(&self.connection).reader().read(&mut chunk) {
                // Zero bytes is the peer's close_notify: a clean end.
                Ok(0) => return Ok(()),
                Ok(n) => {
                    self.buffer.extend_from_slice(&chunk[..n]);
                    return Ok(());
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
            if self.pump()? {
                // The socket closed with no close_notify: the peer went away, which for a
                // request stream is the same end.
                return Ok(());
            }
        }
    }
}

impl Read for TlsReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(out.len());
        out[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for TlsReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.position >= self.buffer.len() {
            self.refill()?;
        }
        Ok(&self.buffer[self.position..])
    }

    fn consume(&mut self, amount: usize) {
        self.position = (self.position + amount).min(self.buffer.len());
    }
}

/// What a node writes replies to: encrypts them and sends them.
pub(crate) struct TlsWriter {
    connection: Shared,
    socket: TcpStream,
}

impl TlsWriter {
    /// A way to end the connection politely (a TLS `close_notify`, then closing the socket),
    /// usable after the writer has gone to the node.
    pub(crate) fn closer(&self) -> io::Result<TlsCloser> {
        Ok(TlsCloser {
            connection: Arc::clone(&self.connection),
            socket: self.socket.try_clone()?,
        })
    }
}

impl Write for TlsWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut connection = lock(&self.connection);
        let written = connection.writer().write(data)?;
        send_pending(&mut connection, &self.socket)?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Every write is sent as it is made.
        Ok(())
    }
}

/// Ends a TLS connection from outside its reader and writer.
pub(crate) struct TlsCloser {
    connection: Shared,
    socket: TcpStream,
}

impl TlsCloser {
    /// Says goodbye to the peer, if it is still listening within a short wait, and closes.
    pub(crate) fn close(&self) {
        let _ = self.socket.set_write_timeout(Some(Duration::from_secs(1)));
        let mut connection = lock(&self.connection);
        connection.send_close_notify();
        let _ = send_pending(&mut connection, &self.socket);
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}
