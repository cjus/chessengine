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
//!    **Close that connection as soon as the fingerprint is read.** A person takes longer than
//!    the node's first-message deadline ([`crate::HANDSHAKE_TIMEOUT`], ten seconds) to compare
//!    and type, and the node hangs up on a connection that sends nothing in that time.
//! 4. The client connects again with `pin: Some(fingerprint)` (so a node showing any other
//!    certificate fails the handshake before the code is sent) and at once calls
//!    [`Connection::pair`] with the code and a name for itself. The node approves the client's
//!    own fingerprint and says so, and closes the connection.
//! 5. From then on the client connects with `pin: Some(fingerprint)` and goes straight to the
//!    protocol: no code, no prompt. A node that shows any other certificate is refused.

use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};

use crate::discovery::{self, Announcement};
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

    /// Sends bytes as they are, with no newline added: for a client that sends a message in
    /// pieces, and for tests of one that does.
    pub fn send_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        let stream = self.stream.get_mut();
        stream.write_all(bytes)?;
        stream.flush()
    }

    /// Sends one line (a CNP message, or a pairing request).
    pub fn send_line(&mut self, line: &str) -> io::Result<()> {
        let stream = self.stream.get_mut();
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()
    }

    /// Splits the connection into the two halves of a client that reads and writes at once: the
    /// node's decrypted output, to read lines from on a thread of its own, and where to write
    /// requests, from another. (One `Connection` can't do both, as a reader holding it would
    /// stop a write; a search can run for a long time, and a request to stop it must not wait.)
    ///
    /// Reads wait as long as the node takes, with no timeout, because the node has no idle
    /// timeout and a search is silent. Reading never holds what writing needs. **Dropping either
    /// half ends the connection**: it sends a TLS `close_notify` and closes the socket, which
    /// ends a search the node is running, and the other half then sees the end (a read returns
    /// no more bytes; a write fails). Drop both when finished.
    ///
    /// Lines already read ahead by [`read_line`](Connection::read_line) are not lost.
    pub fn into_halves(
        self,
    ) -> io::Result<(
        impl BufRead + Send + 'static,
        impl Write + Send + Sync + 'static,
    )> {
        let ahead = self.stream.buffer().to_vec();
        let StreamOwned { conn, sock } = self.stream.into_inner();
        let (reader, writer) = tls::split_client(conn, sock)?;
        let closer = writer.closer()?;
        let reader = Half {
            inner: Cursor::new(ahead).chain(reader),
            closer: writer.closer()?,
        };
        let writer = Half {
            inner: writer,
            closer,
        };
        Ok((reader, writer))
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
        let request = serde_json::json!({ "cnp": "pair", "code": code, "name": name });
        self.send_line(&request.to_string())?;
        let answer = self.read_line()?;
        Ok(answer.is_some_and(|line| {
            serde_json::from_str::<serde_json::Value>(&line).is_ok_and(|v| v["cnp"] == "paired")
        }))
    }
}

/// One half of a split connection: dropping it ends the connection politely.
struct Half<T> {
    inner: T,
    closer: tls::TlsCloser,
}

impl<T> Drop for Half<T> {
    fn drop(&mut self) {
        self.closer.close();
    }
}

impl<T: Read> Read for Half<T> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.inner.read(out)
    }
}

impl<T: BufRead> BufRead for Half<T> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

impl<T: Write> Write for Half<T> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.inner.write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// An engine that answered a discovery probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Where to connect: the address the reply came from, with the port it named.
    pub address: SocketAddr,
    /// What the engine said about itself.
    pub announcement: Announcement,
}

/// Looks for engines on the local network: probes every interface's broadcast address, and
/// `255.255.255.255`, on the standard discovery port, and collects the replies for `wait`.
///
/// **A reply is a hint, not a credential.** Anyone on the network can send one, so connect with
/// the fingerprint it names as a *claim*: a client that has paired with that engine checks it
/// against the fingerprint it pinned, and a new one shows it to the person to confirm before
/// pairing. See the module docs of [`crate::discovery`] for where a broadcast does not reach;
/// a client always offers typing an address as well.
pub fn discover(wait: Duration) -> io::Result<Vec<Found>> {
    discover_at(&broadcast_targets(discovery::PORT), wait)
}

/// The addresses a probe goes to: each interface's broadcast address, and the all-ones address
/// (which on some systems reaches one interface only, hence the others).
fn broadcast_targets(port: u16) -> Vec<SocketAddr> {
    let mut targets: Vec<Ipv4Addr> = vec![Ipv4Addr::BROADCAST];
    for interface in if_addrs::get_if_addrs().unwrap_or_default() {
        if let if_addrs::IfAddr::V4(v4) = interface.addr
            && !v4.ip.is_loopback()
            && let Some(broadcast) = v4.broadcast
            && !targets.contains(&broadcast)
        {
            targets.push(broadcast);
        }
    }
    targets
        .into_iter()
        .map(|ip| SocketAddr::from((ip, port)))
        .collect()
}

/// [`discover`], probing exactly these addresses (a test's loopback engine, or an address a
/// person typed to check whether an engine answers there).
pub fn discover_at(targets: &[SocketAddr], wait: Duration) -> io::Result<Vec<Found>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    let probe = discovery::probe();
    let started = Instant::now();
    let deadline = started + wait;
    let send = |socket: &UdpSocket| {
        for target in targets {
            // A network with no route to one of them (an interface that just went down) is
            // not a reason to stop looking on the others.
            let _ = socket.send_to(&probe, target);
        }
    };
    send(&socket);
    // UDP loses datagrams; a second probe halfway, well inside an engine's rate limit, makes
    // one lost datagram unlikely to hide an engine.
    let mut resent = false;
    let mut found: Vec<Found> = Vec::new();
    let mut buffer = [0u8; discovery::PROBE_BYTES + 1];
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(found);
        }
        if !resent && now - started >= wait / 2 {
            send(&socket);
            resent = true;
        }
        let next = if resent { deadline } else { started + wait / 2 };
        socket.set_read_timeout(Some(
            next.saturating_duration_since(now)
                .max(Duration::from_millis(1)),
        ))?;
        let Ok((n, from)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        let Some(announcement) = Announcement::parse(&buffer[..n]) else {
            continue;
        };
        let address = SocketAddr::new(from.ip(), announcement.port);
        if !found
            .iter()
            .any(|f| f.address == address && f.announcement.fingerprint == announcement.fingerprint)
        {
            found.push(Found {
                address,
                announcement,
            });
        }
    }
}
