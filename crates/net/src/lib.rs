//! # cb2026-net: cb2026 over a network
//!
//! An opt-in listener that serves the **Chess Node Protocol** (CNP) over TCP, so a client on
//! another computer can use this one's processing power. It is a thin adapter: every message
//! goes to the same CNP node (`cb2026-cnp`) that serves a program started on standard input and
//! output, so a search means the same thing either way. This crate only decides who may talk to
//! the node, when, and for how long. `docs/network-play.md` has the whole design; this is its
//! second step.
//!
//! ## What this step does, and what it doesn't
//!
//! - **Loopback only, and no encryption.** [`Server::bind`] refuses any address that isn't
//!   loopback ([`Error::NotLoopback`]), because nothing is encrypted or authenticated yet. TLS
//!   and pairing are step 3, and the listener serves other machines only after them.
//! - **One client at a time.** While a client is connected, another connection is told `Busy`
//!   (CNP's error, with a null id) and closed. The next client gets a **fresh engine**: no hash
//!   table, game or option of the last one survives, so a client can't see or affect another's
//!   state.
//! - **A disconnect cancels the search.** When the connection ends, for any reason, the node
//!   cancels what is running and the session is over; nothing resumes on a later connection.
//! - **A client's `shutdown` and `exit` end its own session**, never the listener. Only the
//!   process that runs the listener stops it ([`StopHandle`], or ending the program).
//! - **Messages are bounded** to [`MESSAGE_BYTES`] (1 MiB; a stdio node reads up to 16 MiB).
//!   CNP's reader refuses a longer line, so a client can't make the listener buffer without end,
//!   and the node tells the client the limit in `initialize`.
//!
//! ## Deadlines
//!
//! - **A new connection must send its first message within [`Config::handshake_timeout`].** A
//!   client that connects and says nothing, or trickles a byte at a time, is dropped. The deadline
//!   is on the whole message, not on each read.
//! - **A slow reader can't stall a search.** A write that doesn't finish within
//!   [`Config::write_timeout`] fails, and the node, which ignores a failed write, carries on.
//! - **A silent connection is not closed for being idle**, because a client waiting for a long
//!   search says nothing for as long as the search runs, and the listener can't tell that from
//!   idleness. A client that has *vanished* (no goodbye, as when its network drops) is found by
//!   TCP keepalive instead, after about [`Config::keepalive`] plus a few probes, so it doesn't hold
//!   the one place for ever.
//!
//! ## Pitfalls
//!
//! - **Accepted sockets may inherit the listener's non-blocking mode** (BSD and macOS do; Linux
//!   doesn't). The listener polls with a non-blocking socket, so every accepted stream is set
//!   blocking again explicitly.
//! - **A read timeout surfaces as `WouldBlock` on some systems and `TimedOut` on others**, so
//!   nothing here matches on its kind; any read error ends the session.
//! - **Stopping needs the connection closed from this side.** A session blocked in a read only
//!   wakes when its socket is shut down, so [`Server::serve`] does that to the active client when
//!   it is told to stop.

#![forbid(unsafe_code)]

use std::fmt;
use std::io::{self, BufRead, BufReader, Read};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cb2026_engine::Engine;
use cnp::stdio::write_message;
use cnp::{Response, ResponseError, code};
use socket2::{SockRef, TcpKeepalive};

/// The port `cb2026 --serve` listens on when none is given. Provisional: it is settled with
/// discovery (step 4 of `docs/network-play.md`), which needs one fixed port of its own.
pub const DEFAULT_PORT: u16 = 4026;

/// How long a new connection has to send its first message.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long one write to a client may take before it fails.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a connection is silent before TCP starts probing it.
pub const KEEPALIVE: Duration = Duration::from_secs(60);

/// The longest message a client may send: far more than a position with its whole history, a
/// search's limits and its options, which come to a few kilobytes.
pub const MESSAGE_BYTES: u64 = 1024 * 1024;

/// The gap between TCP's probes of a silent connection.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// How long a refused connection (told `Busy`) has to take the one line it is sent.
const REFUSAL_TIMEOUT: Duration = Duration::from_secs(1);

/// How often the listener looks for a connection, a finished session or a stop.
const ACCEPT_POLL: Duration = Duration::from_millis(20);

/// How the listener is set up.
#[derive(Clone, Debug)]
pub struct Config {
    /// The address and port to listen on. Loopback only, until TLS (see the crate docs).
    pub address: SocketAddr,
    /// The binary's licence as a whole (an SPDX expression), which the node reports in
    /// `initialize`, as [`cb2026_cnp::Config::license`] does.
    pub license: &'static str,
    /// How long a new connection has to send its first message.
    pub handshake_timeout: Duration,
    /// How long one write to a client may take.
    pub write_timeout: Duration,
    /// How long a connection is silent before TCP probes whether the client is still there.
    pub keepalive: Duration,
    /// The longest message a client may send, in bytes. A longer one is refused, so a client
    /// can't make the listener buffer without end. The node tells the client the limit.
    pub message_bytes: u64,
}

impl Config {
    /// A listener on `address` with the default deadlines.
    pub fn new(address: SocketAddr, license: &'static str) -> Config {
        Config {
            address,
            license,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            write_timeout: WRITE_TIMEOUT,
            keepalive: KEEPALIVE,
            message_bytes: MESSAGE_BYTES,
        }
    }
}

/// Why a listener couldn't start.
#[derive(Debug)]
pub enum Error {
    /// The address isn't loopback, and nothing here is encrypted yet.
    NotLoopback(SocketAddr),
    /// The socket couldn't be opened.
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotLoopback(address) => write!(
                f,
                "{address} isn't a loopback address: nothing is encrypted yet, so cb2026 \
                 listens on 127.0.0.1 or ::1 only"
            ),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

/// Stops a running [`Server`]: it closes the connected client, ending its search, and returns.
#[derive(Clone, Debug)]
pub struct StopHandle(Arc<AtomicBool>);

impl StopHandle {
    /// Asks the listener to stop. It does within a few milliseconds.
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// A CNP listener, bound but not yet serving.
#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    config: Config,
    stop: Arc<AtomicBool>,
}

/// The client being served, and the thread serving it.
struct Session {
    handle: JoinHandle<()>,
    /// A second handle on the connection, to close it from the listener's side.
    stream: TcpStream,
}

impl Server {
    /// Opens the listening socket, or says why not.
    pub fn bind(config: Config) -> Result<Server, Error> {
        if !config.address.ip().is_loopback() {
            return Err(Error::NotLoopback(config.address));
        }
        let listener = TcpListener::bind(config.address)?;
        // Non-blocking, so the listener can notice a stop between connections.
        listener.set_nonblocking(true)?;
        Ok(Server {
            listener,
            config,
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The address the listener is on (with the port the system chose, if `0` was asked for).
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// A handle that stops [`Server::serve`] from another thread.
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle(Arc::clone(&self.stop))
    }

    /// Serves clients, one at a time, until stopped. Each gets the engine `make_engine` builds,
    /// so the engine's settings (threads, hash size, tablebases) are the caller's.
    ///
    /// Returns when [`StopHandle::stop`] is called, after closing the connected client and
    /// waiting for its search to end; or at once with the error if the socket fails for a reason
    /// that isn't a single bad connection.
    pub fn serve(self, make_engine: impl Fn() -> Engine) -> io::Result<()> {
        let mut active: Option<Session> = None;
        let outcome = loop {
            if self.stop.load(Ordering::Relaxed) {
                break Ok(());
            }
            if let Some(done) = active.take_if(|s| s.handle.is_finished()) {
                // A session that failed leaves nothing to report: the node already answered.
                let _ = done.handle.join();
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    // Some systems hand over the listener's non-blocking mode; a session reads
                    // and writes with deadlines, not by polling.
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    if active.is_some() {
                        refuse(stream);
                    } else {
                        active = self.start_session(stream, make_engine());
                    }
                }
                Err(e) if is_passing(&e) => thread::sleep(ACCEPT_POLL),
                Err(e) => break Err(e),
            }
        };
        if let Some(session) = active.take() {
            // Wakes the session's blocked read, which cancels its search and ends it.
            let _ = session.stream.shutdown(Shutdown::Both);
            let _ = session.handle.join();
        }
        outcome
    }

    /// Starts serving `stream` on a thread of its own, or drops it if it can't be set up.
    fn start_session(&self, stream: TcpStream, engine: Engine) -> Option<Session> {
        let closer = stream.try_clone().ok()?;
        let config = self.config.clone();
        let handle = thread::spawn(move || serve_connection(stream, engine, &config));
        Some(Session {
            handle,
            stream: closer,
        })
    }
}

/// Whether an `accept` error is one more wait or one connection that went away, not the
/// listener failing.
fn is_passing(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted | io::ErrorKind::ConnectionAborted
    )
}

/// Tells a connection that arrived while another client is served that cb2026 is busy, then
/// closes it. CNP's `Busy`, with a null id since no request was read.
fn refuse(stream: TcpStream) {
    let _ = stream.set_write_timeout(Some(REFUSAL_TIMEOUT));
    let error = ResponseError::new(
        code::BUSY,
        "this cb2026 is serving another client; it serves one at a time",
    );
    let mut stream = stream;
    let _ = write_message(&mut stream, &Response::error(None, error).into());
    let _ = stream.shutdown(Shutdown::Both);
}

/// Serves one client until the connection ends, the client says `exit`, or its deadlines pass.
fn serve_connection(stream: TcpStream, engine: Engine, config: &Config) {
    // A client that vanished is found by keepalive; a few probes 10 seconds apart decide.
    let keepalive = TcpKeepalive::new()
        .with_time(config.keepalive)
        .with_interval(KEEPALIVE_INTERVAL);
    let _ = SockRef::from(&stream).set_tcp_keepalive(&keepalive);
    // A request or a reply is small and the client waits for it, so don't hold it back.
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(config.write_timeout));
    let (Ok(control), Ok(writer), Ok(closer)) =
        (stream.try_clone(), stream.try_clone(), stream.try_clone())
    else {
        return;
    };
    let input = FirstMessage::new(stream, control, config.handshake_timeout);
    // Whatever ends it (the client's goodbye, a failed read, a deadline), the node has by then
    // cancelled what it ran, so there is nothing to report.
    let _ = cb2026_cnp::run(
        input,
        Box::new(writer),
        None,
        engine,
        cb2026_cnp::Config {
            license: config.license,
            message_bytes: Some(config.message_bytes),
        },
    );
    // The client said `exit` or went wrong; close the connection so it isn't left hanging.
    let _ = closer.shutdown(Shutdown::Both);
}

/// A connection's input, with a deadline on its first message.
///
/// A client that connects and says nothing, or sends a byte at a time, would otherwise hold the
/// one place. Until a complete line has arrived, each wait for bytes is limited to what is left
/// of one deadline for the whole message (not a fresh timeout per read, which a trickle of
/// bytes would keep renewing). After the first line, reads wait as long as the client likes: see
/// the crate docs, "Deadlines".
struct FirstMessage {
    input: BufReader<TcpStream>,
    /// Another handle on the same socket, to set its read timeout (the buffered reader owns
    /// the first).
    control: TcpStream,
    /// When the first message is due, or `None` once one has arrived.
    deadline: Option<Instant>,
}

impl FirstMessage {
    fn new(stream: TcpStream, control: TcpStream, within: Duration) -> FirstMessage {
        FirstMessage {
            input: BufReader::new(stream),
            control,
            deadline: Some(Instant::now() + within),
        }
    }
}

impl Read for FirstMessage {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = {
            let buffer = self.fill_buf()?;
            let n = buffer.len().min(out.len());
            out[..n].copy_from_slice(&buffer[..n]);
            n
        };
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for FirstMessage {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if let Some(deadline) = self.deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.control.set_read_timeout(Some(left))?;
        }
        let buffer = self.input.fill_buf()?;
        if self.deadline.is_some() && buffer.contains(&b'\n') {
            // The first message is complete: from here a long silence is a client waiting for a
            // search, not a client that has stalled.
            self.deadline = None;
            self.control.set_read_timeout(None)?;
        }
        Ok(buffer)
    }

    fn consume(&mut self, amount: usize) {
        self.input.consume(amount);
    }
}
