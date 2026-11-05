//! A reference client for the listener: what a program on another computer does to pair with a
//! cb2026 node and then use it. It is small on purpose, so another client (Solrac's, or a script)
//! can follow it, and it is how this crate's tests play the other side.
//!
//! ## Pairing, from the client's side
//!
//! 1. The person starts `cb2026 --pair` on the engine's computer, which shows the engine's
//!    **fingerprint** and an **eight-digit code**.
//! 2. The client connects with [`connect`] and `pin: None`, which accepts whatever certificate
//!    the server shows, and reads [`Connection::server_fingerprint`].
//! 3. **The client shows that fingerprint to the person, who compares it with the one on the
//!    engine's screen.** This is the step that stops someone in the middle: the code is
//!    useless to an attacker who can't also present the engine's certificate. A client must not
//!    send the code until the person has said the two match.
//! 4. The client calls [`Connection::pair`] with the code and a name for itself. The node
//!    approves the client's own fingerprint and says so, and closes the connection.
//! 5. From then on the client connects with `pin: Some(fingerprint)` and goes straight to the
//!    protocol: no code, no prompt. A node that shows any other certificate is refused.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};

use crate::identity::{Fingerprint, Identity};
use crate::tls;

/// How long [`connect`] waits to reach the node and finish the handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A client's end of an encrypted connection to a cb2026 node.
pub struct Connection {
    stream: BufReader<StreamOwned<ClientConnection, TcpStream>>,
    server: Fingerprint,
}

/// Connects to a node and completes the TLS handshake, presenting `identity` as this client.
///
/// With `pin: Some(fingerprint)` the node must show that certificate or the connection fails.
/// With `None` it accepts any, so the caller can read [`Connection::server_fingerprint`] and
/// ask the person to confirm it before pairing; send nothing private on such a connection.
///
/// A node that doesn't approve this client closes the connection after the handshake with no
/// reason, so the first read or write is where that shows.
pub fn connect(
    address: SocketAddr,
    identity: &Identity,
    pin: Option<Fingerprint>,
) -> io::Result<Connection> {
    let socket = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
    socket.set_nodelay(true)?;
    socket.set_read_timeout(Some(CONNECT_TIMEOUT))?;
    let name = ServerName::try_from("cb2026").map_err(io::Error::other)?;
    let mut tls = ClientConnection::new(tls::client_config(identity, pin)?, name)
        .map_err(io::Error::other)?;
    let mut socket = socket;
    while tls.is_handshaking() {
        tls.complete_io(&mut socket)?;
    }
    let server = tls
        .peer_certificates()
        .and_then(|chain| chain.first())
        .map(Fingerprint::of)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "the node sent no certificate")
        })?;
    // After the handshake the client waits as long as a search takes.
    socket.set_read_timeout(None)?;
    Ok(Connection {
        stream: BufReader::new(StreamOwned::new(tls, socket)),
        server,
    })
}

impl Connection {
    /// The fingerprint of the certificate the node showed.
    pub fn server_fingerprint(&self) -> Fingerprint {
        self.server
    }

    /// Sends one line (a CNP message, or a pairing request).
    pub fn send_line(&mut self, line: &str) -> io::Result<()> {
        let stream = self.stream.get_mut();
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()
    }

    /// Reads one line, without its newline, or `None` when the node has closed the connection.
    pub fn read_line(&mut self) -> io::Result<Option<String>> {
        let mut line = String::new();
        match self.stream.read_line(&mut line) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(line.trim_end().to_string())),
            // A node that closes without a goodbye is a node that closed.
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Asks to be approved, with the code the person was shown on the node's screen and a name
    /// for this client (kept in the node's approved list). Whether the node said yes. The
    /// connection is over after this either way.
    pub fn pair(&mut self, code: &str, name: &str) -> io::Result<bool> {
        let request = serde_json::json!({ "cb2026": "pair", "code": code, "name": name });
        self.send_line(&request.to_string())?;
        let answer = self.read_line()?;
        Ok(answer.is_some_and(|line| {
            serde_json::from_str::<serde_json::Value>(&line).is_ok_and(|v| v["cb2026"] == "paired")
        }))
    }
}
