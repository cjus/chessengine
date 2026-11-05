//! End-to-end tests of the listener, over real loopback connections: a client speaks CNP to a
//! [`Server`] running on a thread of its own.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cb2026_engine::Engine;
use cb2026_net::{Config, Error, Server, StopHandle};

/// Long enough for a slow CI runner, short enough that a hung test says so.
const WAIT: Duration = Duration::from_secs(30);

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":"1.0","client":{"name":"test","version":"0"}}}"#;
const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// A listener running on its own thread.
struct Running {
    address: SocketAddr,
    stop: StopHandle,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    fn start() -> Running {
        Running::with(|_| {})
    }

    fn with(adjust: impl FnOnce(&mut Config)) -> Running {
        let mut config = Config::new("127.0.0.1:0".parse().unwrap(), "MIT OR Apache-2.0");
        adjust(&mut config);
        let server = Server::bind(config).expect("binds on loopback");
        let address = server.local_addr().expect("has an address");
        let stop = server.stop_handle();
        let thread = thread::spawn(move || server.serve(Engine::new).expect("serves"));
        Running {
            address,
            stop,
            thread: Some(thread),
        }
    }

    fn connect(&self) -> Client {
        let stream = TcpStream::connect(self.address).expect("connects");
        stream.set_read_timeout(Some(WAIT)).unwrap();
        Client {
            reader: BufReader::new(stream.try_clone().unwrap()),
            stream,
        }
    }

    /// Stops the listener and waits for it to return.
    fn finish(mut self) {
        self.stop.stop();
        self.thread
            .take()
            .unwrap()
            .join()
            .expect("the listener ends cleanly");
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// A client's end of a connection.
struct Client {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Client {
    fn send(&mut self, line: &str) {
        writeln!(self.stream, "{line}").expect("sends");
    }

    /// The next line, or `None` if the connection ended (or nothing came in time).
    fn line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line),
        }
    }

    /// Reads until a line with `needle`, skipping others (progress, say).
    fn until(&mut self, needle: &str) -> String {
        loop {
            let line = self
                .line()
                .unwrap_or_else(|| panic!("no line with {needle}"));
            if line.contains(needle) {
                return line;
            }
        }
    }

    fn initialize(&mut self) {
        self.send(INITIALIZE);
        let answer = self.until(r#""id":1"#);
        assert!(answer.contains(r#""result""#), "{answer}");
    }

    /// Whether the connection has been closed from the other side.
    fn closed(&mut self) -> bool {
        self.line().is_none()
    }
}

/// Connects until a client is served rather than told it is busy: the listener notices a
/// finished session within a few milliseconds.
fn connect_when_free(running: &Running) -> Client {
    let start = Instant::now();
    loop {
        let mut client = running.connect();
        client.send(INITIALIZE);
        match client.line() {
            Some(line) if line.contains(r#""result""#) => return client,
            _ => assert!(start.elapsed() < WAIT, "the place never came free"),
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn initialize_tells_the_client_the_message_limit() {
    let running = Running::start();
    let mut client = running.connect();
    client.send(INITIALIZE);
    let answer = client.until(r#""id":1"#);
    assert!(
        answer.contains(&format!(
            r#""maxMessageBytes":{}"#,
            cb2026_net::MESSAGE_BYTES
        )),
        "{answer}"
    );
    running.finish();
}

#[test]
fn a_client_can_search_over_loopback_and_say_goodbye() {
    let running = Running::start();
    let mut client = running.connect();
    client.initialize();
    client.send(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"engine/search","params":{{"position":{{"fen":"{START}"}},"limits":{{"depth":3}}}}}}"#
    ));
    let answer = client.until(r#""id":2"#);
    assert!(answer.contains("bestMove"), "{answer}");
    client.send(r#"{"jsonrpc":"2.0","id":99,"method":"shutdown"}"#);
    client.until(r#""id":99"#);
    client.send(r#"{"jsonrpc":"2.0","method":"exit"}"#);
    assert!(client.closed(), "the connection closes after exit");
    running.finish();
}

#[test]
fn only_loopback_addresses_are_accepted_until_there_is_encryption() {
    for address in ["0.0.0.0:0", "192.168.1.20:4026", "[::]:0", "8.8.8.8:4026"] {
        let config = Config::new(address.parse().unwrap(), "MIT OR Apache-2.0");
        assert!(
            matches!(Server::bind(config), Err(Error::NotLoopback(_))),
            "{address} was accepted"
        );
    }
    for address in ["127.0.0.1:0", "[::1]:0"] {
        let config = Config::new(address.parse().unwrap(), "MIT OR Apache-2.0");
        // An IPv6 loopback may be missing on a host; it is refused for another reason then.
        assert!(
            !matches!(Server::bind(config), Err(Error::NotLoopback(_))),
            "{address}"
        );
    }
}

#[test]
fn a_second_client_is_told_busy_and_the_first_is_not_disturbed() {
    let running = Running::start();
    let mut first = running.connect();
    first.initialize();
    for _ in 0..20 {
        let mut other = running.connect();
        let line = other.line().expect("a refusal is sent");
        assert!(line.contains(r#""id":null"#), "{line}");
        assert!(line.contains(r#""error""#), "{line}");
        assert!(other.closed(), "a refused connection is closed");
    }
    // The first client's session goes on.
    first.send(r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#);
    first.until(r#""id":5"#);
    running.finish();
}

#[test]
fn a_disconnect_cancels_the_search_and_frees_the_place() {
    let running = Running::start();
    let mut client = running.connect();
    client.initialize();
    client.send(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"engine/search","params":{{"position":{{"fen":"{START}"}},"limits":{{"infinite":true}}}}}}"#
    ));
    // Gone without a word, while the search runs.
    drop(client);
    let mut next = connect_when_free(&running);
    // A new session: its request ids and everything else start afresh.
    next.send(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#);
    next.until(r#""id":7"#);
    running.finish();
}

#[test]
fn a_clients_shutdown_ends_its_session_and_not_the_listener() {
    let running = Running::start();
    let mut client = running.connect();
    client.initialize();
    client.send(r#"{"jsonrpc":"2.0","id":99,"method":"shutdown"}"#);
    client.until(r#""id":99"#);
    client.send(r#"{"jsonrpc":"2.0","method":"exit"}"#);
    assert!(client.closed());
    let mut next = connect_when_free(&running);
    next.send(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#);
    next.until(r#""id":8"#);
    running.finish();
}

#[test]
fn a_silent_connection_is_dropped_at_the_handshake_deadline() {
    let running = Running::with(|c| c.handshake_timeout = Duration::from_millis(300));
    let mut silent = running.connect();
    let start = Instant::now();
    assert!(silent.closed(), "a silent connection is closed");
    assert!(start.elapsed() < Duration::from_secs(10));
    // It didn't keep the place.
    connect_when_free(&running);
    running.finish();
}

#[test]
fn a_trickle_of_bytes_does_not_renew_the_handshake_deadline() {
    let running = Running::with(|c| c.handshake_timeout = Duration::from_millis(400));
    let mut slow = running.connect();
    let start = Instant::now();
    // A byte every 100 ms, never a newline: each read is fast, the message never completes.
    for byte in INITIALIZE.bytes().take(40) {
        if slow.stream.write_all(&[byte]).is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
        if start.elapsed() > Duration::from_secs(3) {
            break;
        }
    }
    assert!(slow.closed(), "the trickle was cut off");
    assert!(start.elapsed() < Duration::from_secs(10));
    running.finish();
}

#[test]
fn a_long_silence_during_a_search_is_not_idleness() {
    // The handshake deadline is gone once the first message has come, so a client waiting
    // for a search that runs longer than it keeps its connection.
    let running = Running::with(|c| c.handshake_timeout = Duration::from_millis(300));
    let mut client = running.connect();
    client.initialize();
    client.send(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"engine/search","params":{{"position":{{"fen":"{START}"}},"limits":{{"infinite":true}}}}}}"#
    ));
    thread::sleep(Duration::from_millis(1200));
    client.send(r#"{"jsonrpc":"2.0","id":3,"method":"engine/stop","params":{"id":2}}"#);
    let answer = client.until(r#""id":2"#);
    assert!(answer.contains(r#""stopped":true"#), "{answer}");
    running.finish();
}

#[test]
fn an_oversized_message_is_refused_and_the_listener_stays_up() {
    let running = Running::start();
    let mut client = running.connect();
    client.initialize();
    // Past the limit, with no newline until the end.
    let huge = "a".repeat(2 * cb2026_net::MESSAGE_BYTES as usize);
    // The node may reply and stop reading before all of it is sent; that is its right.
    let _ = client.stream.write_all(huge.as_bytes());
    let _ = client.stream.write_all(b"\n");
    let answer = client.line().expect("the oversized message is answered");
    assert!(answer.contains(r#""error""#), "{answer}");
    drop(client);
    connect_when_free(&running);
    running.finish();
}

#[test]
fn stopping_the_listener_ends_a_running_search_and_closes_the_client() {
    let running = Running::start();
    let mut client = running.connect();
    client.initialize();
    client.send(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"engine/search","params":{{"position":{{"fen":"{START}"}},"limits":{{"infinite":true}}}}}}"#
    ));
    thread::sleep(Duration::from_millis(200));
    running.finish();
    assert!(client.closed(), "the client is disconnected");
}
