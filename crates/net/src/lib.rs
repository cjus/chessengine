//! # cb2026-net: cb2026 over a network
//!
//! An opt-in listener that serves the **Chess Node Protocol** (CNP) over TCP, so a client on
//! another computer can use this one's processing power. It is a thin adapter: every message
//! goes to the same CNP node (`cb2026-cnp`) that serves a program started on standard input and
//! output, so a search means the same thing either way. This crate only decides who may talk to
//! the node, when, and for how long. `docs/network-play.md` has the whole design; this is its
//! second and third steps.
//!
//! ## Two ways to run it
//!
//! - **With trust ([`Config::trust`]): TLS 1.3, and only approved clients.** This is how another
//!   computer connects. Each side has a self-signed certificate ([`Identity`]) and recognises the
//!   other by its fingerprint. A client the node hasn't approved may only *pair*: send the
//!   one-time code the person was shown by `cb2026 --pair`, after confirming the node's
//!   fingerprint. Then it is approved (see [`trust`]) and connects with no code from then on.
//!   Anyone else is closed on, with nothing said about why.
//! - **Without trust: plaintext, loopback only.** [`Server::bind`] refuses any address that isn't
//!   loopback ([`Error::NotLoopback`]) unless there is trust, because nothing then is encrypted
//!   or authenticated. It is for a program on this computer, and for tests.
//!
//! Either way, what follows holds.
//!
//! ## What the listener promises
//!
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
//! - **A new connection must finish its handshake and send its first message within
//!   [`Config::handshake_timeout`].** A client that connects and says nothing, or trickles a
//!   byte at a time, is dropped. The deadline is on the whole of it, not on each read.
//! - **Strangers don't take the place.** A connection is greeted on a thread of its own
//!   (at most [`MAX_GREETINGS`] at once; more are dropped unanswered). Only a client that is
//!   approved, or any client of a plaintext loopback listener, gets as far as asking for the
//!   one place, so a stranger can't keep an approved client out by connecting.
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

mod abuse;
pub mod client;
mod deadline;
pub mod discovery;
mod files;
mod identity;
mod tls;
pub mod trust;

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use cb2026_engine::Engine;
use cnp::stdio::write_message;
use cnp::{Response, ResponseError, code};
use rustls::ServerConfig;
use socket2::{SockRef, TcpKeepalive};

use crate::abuse::Abuse;
pub use crate::abuse::{BAN, FAILURES, PER_ADDRESS, WINDOW};
use crate::deadline::{Deadline, Gate};
pub use crate::files::default_dir;
pub use crate::identity::{BadFingerprint, Fingerprint, Identity};
use crate::trust::Paired;
pub use crate::trust::Store;

/// The port `cb2026 --serve` listens on when none is given. Provisional: it is settled with
/// discovery (step 4 of `docs/network-play.md`), which needs one fixed port of its own.
pub const DEFAULT_PORT: u16 = 4026;

/// How long a new connection has to finish its handshake and send its first message.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long one write to a client may take before it fails.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a connection is silent before TCP starts probing it.
pub const KEEPALIVE: Duration = Duration::from_secs(60);

/// The longest message a client may send: far more than a position with its whole history, a
/// search's limits and its options, which come to a few kilobytes.
pub const MESSAGE_BYTES: u64 = 1024 * 1024;

/// How many new connections are greeted (handshake, approval check, pairing) at once. More are
/// dropped without an answer, so a flood can't make the listener run threads without end.
pub const MAX_GREETINGS: usize = 16;

/// The longest pairing request a client may send, in bytes: a code, a name and a few words.
const PAIRING_BYTES: u64 = 1024;

/// The gap between TCP's probes of a silent connection.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// How long a refused connection (told `Busy`) has to take the one line it is sent.
const REFUSAL_TIMEOUT: Duration = Duration::from_secs(1);

/// How often the listener looks for a connection, a finished session or a stop.
const ACCEPT_POLL: Duration = Duration::from_millis(20);

/// What lets a listener serve other computers: its own identity, and the list of clients it has
/// approved. Without it a listener is plaintext and loopback only.
pub struct Trust {
    store: Store,
    identity: Identity,
    tls: Arc<ServerConfig>,
}

impl fmt::Debug for Trust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Trust")
            .field("dir", &self.store.dir())
            .field("fingerprint", &self.identity.fingerprint())
            .finish_non_exhaustive()
    }
}

impl Trust {
    /// Opens the trust files in `store`'s directory, making this node's identity (a certificate
    /// and key) if it has none yet.
    pub fn open(store: Store) -> io::Result<Trust> {
        let identity = store.identity()?;
        let tls = tls::server_config(&identity)?;
        Ok(Trust {
            store,
            identity,
            tls,
        })
    }

    /// The fingerprint a person compares when pairing a client.
    pub fn fingerprint(&self) -> Fingerprint {
        self.identity.fingerprint()
    }

    /// The approved clients and the pairing window.
    pub fn store(&self) -> &Store {
        &self.store
    }
}

/// How the listener is set up.
#[derive(Clone, Debug)]
pub struct Config {
    /// The address and port to listen on. Loopback only, unless there is [`Config::trust`].
    pub address: SocketAddr,
    /// The binary's licence as a whole (an SPDX expression), which the node reports in
    /// `initialize`, as [`cb2026_cnp::Config::license`] does.
    pub license: &'static str,
    /// Encryption and approved clients, or `None` for plaintext on loopback.
    pub trust: Option<Arc<Trust>>,
    /// Answer probes from clients looking for engines on the local network (`--discover`), or
    /// `None` to answer none. Needs [`Config::trust`].
    pub discovery: Option<Discovery>,
    /// How long a new connection has to finish its handshake and send its first message.
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
    /// A plaintext, loopback listener on `address` with the default deadlines. Set
    /// [`Config::trust`] to serve other computers.
    pub fn new(address: SocketAddr, license: &'static str) -> Config {
        Config {
            address,
            license,
            trust: None,
            discovery: None,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            write_timeout: WRITE_TIMEOUT,
            keepalive: KEEPALIVE,
            message_bytes: MESSAGE_BYTES,
        }
    }
}

/// How the listener answers clients looking for it (see [`discovery`]).
#[derive(Clone, Debug)]
pub struct Discovery {
    /// The UDP port to listen for probes on: [`discovery::PORT`] unless a test needs another.
    pub port: u16,
    /// The name the owner gave this node, shown to a person choosing among engines. Cleaned by
    /// [`discovery::clean_name`] when used.
    pub name: String,
}

impl Discovery {
    /// Discovery on the standard port, under `name`.
    pub fn new(name: impl Into<String>) -> Discovery {
        Discovery {
            port: discovery::PORT,
            name: name.into(),
        }
    }
}

/// Why a listener couldn't start.
#[derive(Debug)]
pub enum Error {
    /// The address isn't loopback, and there is no TLS to protect it.
    NotLoopback(SocketAddr),
    /// Discovery was asked for on a listener with no TLS, which nothing outside this computer
    /// should be told about.
    DiscoveryWithoutTrust,
    /// The socket couldn't be opened.
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotLoopback(address) => write!(
                f,
                "{address} isn't a loopback address: without TLS and pairing, cb2026 \
                 listens on 127.0.0.1 or ::1 only"
            ),
            Error::DiscoveryWithoutTrust => f.write_str(
                "discovery needs the encrypted listener (an address beyond loopback, or --tls)",
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
    /// The socket probes arrive on, if discovery is on.
    probes: Option<UdpSocket>,
    config: Config,
    stop: Arc<AtomicBool>,
}

/// A client that has been greeted and may ask for the one place: its input and output, and the
/// means of ending the connection.
struct Link {
    input: FirstMessage,
    output: Box<dyn Write + Send>,
    /// Says goodbye the way the connection allows (a TLS `close_notify`) and closes it.
    goodbye: Box<dyn FnOnce() + Send>,
    /// A handle on the socket, to close it abruptly when the listener stops.
    socket: TcpStream,
}

/// What a greeting ends in.
enum Arrival {
    /// A client that may ask for the one place.
    Link(Link),
    /// A connection that was not let in, from this address: a failed handshake, a stranger.
    Failed(IpAddr),
}

/// The client being served, and the thread serving it.
struct Session {
    handle: JoinHandle<()>,
    /// A second handle on the connection, to close it from the listener's side.
    socket: TcpStream,
}

/// A connection being greeted, and the thread greeting it.
struct Greeting {
    handle: JoinHandle<()>,
    socket: TcpStream,
    address: IpAddr,
}

impl Server {
    /// Opens the listening socket, or says why not.
    pub fn bind(config: Config) -> Result<Server, Error> {
        if config.trust.is_none() && !config.address.ip().is_loopback() {
            return Err(Error::NotLoopback(config.address));
        }
        if config.discovery.is_some() && config.trust.is_none() {
            return Err(Error::DiscoveryWithoutTrust);
        }
        let listener = TcpListener::bind(config.address)?;
        // Non-blocking, so the listener can notice a stop between connections.
        listener.set_nonblocking(true)?;
        // Opened here, so a port already in use is reported before anything is announced.
        let probes = match &config.discovery {
            Some(discovery) => Some(discovery::bind(discovery.port)?),
            None => None,
        };
        Ok(Server {
            listener,
            probes,
            config,
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The address the listener is on (with the port the system chose, if `0` was asked for).
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// The address probes are answered on, if discovery is on.
    pub fn discovery_addr(&self) -> Option<SocketAddr> {
        self.probes.as_ref().and_then(|s| s.local_addr().ok())
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
        let responder = match (
            self.probes.as_ref(),
            &self.config.discovery,
            &self.config.trust,
        ) {
            (Some(socket), Some(discovery), Some(trust)) => {
                let who = discovery::Who {
                    name: discovery::clean_name(&discovery.name),
                    tcp_port: self.listener.local_addr()?.port(),
                    trust: Arc::clone(trust),
                };
                Some(discovery::Responder::start(
                    socket.try_clone()?,
                    who,
                    Arc::clone(&self.stop),
                ))
            }
            _ => None,
        };
        let (ready, arrivals) = mpsc::channel::<Arrival>();
        let mut abuse = Abuse::default();
        let mut active: Option<Session> = None;
        let mut greeting: Vec<Greeting> = Vec::new();
        let outcome = loop {
            if self.stop.load(Ordering::Relaxed) {
                break Ok(());
            }
            if let Some(done) = active.take_if(|s| s.handle.is_finished()) {
                // A session that failed leaves nothing to report: the node already answered.
                let _ = done.handle.join();
            }
            reap(&mut greeting);
            // Clients that are greeted ask for the one place in the order they arrive here.
            while let Ok(arrival) = arrivals.try_recv() {
                match arrival {
                    Arrival::Link(link) if active.is_none() => {
                        active = self.start_session(link, make_engine());
                    }
                    Arrival::Link(link) => refuse(link),
                    Arrival::Failed(ip) => abuse.failed(ip, Instant::now()),
                }
            }
            match self.listener.accept() {
                Ok((stream, peer)) => {
                    // Some systems hand over the listener's non-blocking mode; a connection is
                    // read and written with deadlines, not by polling.
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    if greeting.len() >= MAX_GREETINGS {
                        // Over the limit: closed without a word, which costs nothing.
                        continue;
                    }
                    let in_flight = greeting.iter().filter(|g| g.address == peer.ip()).count();
                    if !abuse.may_greet(peer.ip(), in_flight, Instant::now()) {
                        // One address asking too much: closed without a word.
                        continue;
                    }
                    if let Some(started) = self.start_greeting(stream, peer.ip(), ready.clone()) {
                        greeting.push(started);
                    }
                }
                Err(e) if is_passing(&e) => thread::sleep(ACCEPT_POLL),
                Err(e) => break Err(e),
            }
        };
        // Closing the sockets wakes every thread that is blocked reading one.
        for g in &greeting {
            let _ = g.socket.shutdown(Shutdown::Both);
        }
        if let Some(session) = &active {
            let _ = session.socket.shutdown(Shutdown::Both);
        }
        for g in greeting {
            let _ = g.handle.join();
        }
        if let Some(session) = active {
            let _ = session.handle.join();
        }
        if let Some(responder) = responder {
            responder.join();
        }
        // Any client that was greeted but not yet served is closed with its link.
        while let Ok(arrival) = arrivals.try_recv() {
            if let Arrival::Link(link) = arrival {
                let _ = link.socket.shutdown(Shutdown::Both);
            }
        }
        outcome
    }

    /// Greets `stream` on a thread of its own, or drops it if it can't be set up.
    fn start_greeting(
        &self,
        stream: TcpStream,
        address: IpAddr,
        ready: Sender<Arrival>,
    ) -> Option<Greeting> {
        let closer = stream.try_clone().ok()?;
        let config = self.config.clone();
        let handle = thread::spawn(move || {
            let arrival = match greet(stream, &config) {
                Greeted::Link(link) => Arrival::Link(link),
                Greeted::Paired => return,
                Greeted::Refused => Arrival::Failed(address),
            };
            let _ = ready.send(arrival);
        });
        Some(Greeting {
            handle,
            socket: closer,
            address,
        })
    }

    /// Starts serving a greeted client on a thread of its own.
    fn start_session(&self, link: Link, engine: Engine) -> Option<Session> {
        let socket = link.socket.try_clone().ok()?;
        let license = self.config.license;
        let message_bytes = self.config.message_bytes;
        let handle = thread::spawn(move || {
            // Whatever ends it (the client's goodbye, a failed read, a deadline), the node has by
            // then cancelled what it ran, so there is nothing to report.
            let _ = cb2026_cnp::run(
                link.input,
                link.output,
                None,
                engine,
                cb2026_cnp::Config {
                    license,
                    message_bytes: Some(message_bytes),
                },
            );
            // The client said `exit` or went wrong; end the connection so it isn't left hanging.
            (link.goodbye)();
        });
        Some(Session { handle, socket })
    }
}

/// Joins the greetings that have finished.
fn reap(greeting: &mut Vec<Greeting>) {
    let mut i = 0;
    while i < greeting.len() {
        if greeting[i].handle.is_finished() {
            let _ = greeting.swap_remove(i).handle.join();
        } else {
            i += 1;
        }
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

/// Tells a client that arrived while another is served that cb2026 is busy, then closes the
/// connection. CNP's `Busy`, with a null id since no request was read. Only a client that is
/// approved gets this far, so what it learns (that the node is in use) is nothing a stranger can
/// ask.
fn refuse(link: Link) {
    let _ = link.socket.set_write_timeout(Some(REFUSAL_TIMEOUT));
    let error = ResponseError::new(
        code::BUSY,
        "this cb2026 is serving another client; it serves one at a time",
    );
    let mut output = link.output;
    let _ = write_message(&mut output, &Response::error(None, error).into());
    (link.goodbye)();
}

/// How greeting a connection ended.
enum Greeted {
    /// A client that may ask for the one place.
    Link(Link),
    /// A stranger that paired: approved now, and closed (it connects again as a client).
    Paired,
    /// Anything else: the connection is closed.
    Refused,
}

/// Greets a new connection: sets up the socket, and (with trust) completes the TLS handshake,
/// checks whether the client is approved, and lets one that isn't pair. The link of a client that
/// may ask for the one place, a stranger that paired, or a connection that wasn't let in. A
/// stranger is told nothing: its connection is closed.
fn greet(stream: TcpStream, config: &Config) -> Greeted {
    greet_with(stream, config).unwrap_or(Greeted::Refused)
}

fn greet_with(stream: TcpStream, config: &Config) -> Option<Greeted> {
    // A client that vanished is found by keepalive; a few probes 10 seconds apart decide.
    let keepalive = TcpKeepalive::new()
        .with_time(config.keepalive)
        .with_interval(KEEPALIVE_INTERVAL);
    let _ = SockRef::from(&stream).set_tcp_keepalive(&keepalive);
    // A request or a reply is small and the client waits for it, so don't hold it back.
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(config.write_timeout));
    let deadline = Deadline::after(config.handshake_timeout);
    let gate = Gate::new(stream.try_clone().ok()?, deadline.clone());
    let control = stream.try_clone().ok()?;
    let socket = stream.try_clone().ok()?;
    let Some(trust) = &config.trust else {
        let writer = stream.try_clone().ok()?;
        let closer = stream.try_clone().ok()?;
        return Some(Greeted::Link(Link {
            input: FirstMessage::new(Box::new(BufReader::new(gate)), control, deadline),
            output: Box::new(writer),
            goodbye: Box::new(move || {
                let _ = closer.shutdown(Shutdown::Both);
            }),
            socket,
        }));
    };
    let (reader, mut writer, fingerprint) =
        tls::accept(gate, stream, Arc::clone(&trust.tls)).ok()?;
    let goodbye = writer.closer().ok()?;
    let mut input = FirstMessage::new(Box::new(reader), control, deadline);
    if !trust.store.is_approved(&fingerprint) {
        // A client that isn't approved may only ask to pair, and only while a window is open.
        let paired = pair(trust, fingerprint, &mut input, &mut writer);
        goodbye.close();
        return Some(if paired {
            Greeted::Paired
        } else {
            Greeted::Refused
        });
    }
    Some(Greeted::Link(Link {
        input,
        output: Box::new(writer),
        goodbye: Box::new(move || goodbye.close()),
        socket,
    }))
}

/// Handles the one message an unapproved client may send: a request to pair. Approves the client
/// and says so if the code is right; otherwise says nothing (the caller closes the connection),
/// so a refusal never tells a guesser what was wrong.
fn pair(
    trust: &Trust,
    fingerprint: Fingerprint,
    input: &mut FirstMessage,
    output: &mut impl Write,
) -> bool {
    let mut line = String::new();
    // Bounded: a client with no business here can't make the node buffer more than this.
    let read = input.by_ref().take(PAIRING_BYTES).read_line(&mut line);
    if read.is_err() || !line.ends_with('\n') {
        return false;
    }
    let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) else {
        return false;
    };
    if request["cnp"] != "pair" {
        return false;
    }
    let (Some(code), name) = (
        request["code"].as_str(),
        request["name"].as_str().unwrap_or_default(),
    ) else {
        return false;
    };
    let paired = matches!(
        trust
            .store
            .try_pair(code, fingerprint, name, SystemTime::now()),
        Ok(Paired::Yes)
    );
    if paired {
        let _ = writeln!(output, r#"{{"cnp":"paired"}}"#);
    }
    paired
}

/// A connection's input, with its deadline cleared when the first message is complete.
///
/// The deadline itself is kept at the socket's read (see [`deadline`]); this only notices that a
/// whole line has arrived, and from then on lets a long silence be a client waiting for a search,
/// not a client that has stalled.
struct FirstMessage {
    input: Box<dyn BufRead + Send>,
    /// Another handle on the same socket, to lift its read timeout once the deadline is over.
    control: TcpStream,
    deadline: Deadline,
}

impl FirstMessage {
    fn new(input: Box<dyn BufRead + Send>, control: TcpStream, deadline: Deadline) -> FirstMessage {
        FirstMessage {
            input,
            control,
            deadline,
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
        let buffer = self.input.fill_buf()?;
        if self.deadline.is_set() && buffer.contains(&b'\n') {
            // The first message is complete.
            self.deadline.clear();
            self.control.set_read_timeout(None)?;
        }
        Ok(buffer)
    }

    fn consume(&mut self, amount: usize) {
        self.input.consume(amount);
    }
}
