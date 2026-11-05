//! The negative tests of #209's verification list, run against the listener both ways it runs:
//! plaintext on loopback and TLS with an approved client. Each test says what an attacker, or a
//! careless client, sends, and what must still be true afterwards: the listener is up, the
//! next client is served, nothing stale reaches it, and no thread or memory is held without
//! limit.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use cb2026_engine::Engine;
use cb2026_net::client::{self, Connection};
use cb2026_net::{Config, Identity, Server, StopHandle, Store, Trust};

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":"1.0","client":{"name":"test","version":"0"}}}"#;
const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const WAIT: Duration = Duration::from_secs(30);

fn search(id: u32, limits: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"engine/search","params":{{"position":{{"fen":"{START}"}},"limits":{limits}}}}}"#
    )
}

fn cancel(id: u32) -> String {
    format!(r#"{{"jsonrpc":"2.0","method":"$/cancelRequest","params":{{"id":{id}}}}}"#)
}

#[derive(Clone, Copy, Debug)]
enum Flavor {
    Plain,
    Tls,
}

const BOTH: [Flavor; 2] = [Flavor::Plain, Flavor::Tls];

/// A listener of either flavor on a thread of its own.
struct Node {
    flavor: Flavor,
    address: SocketAddr,
    stop: StopHandle,
    thread: Option<JoinHandle<()>>,
    /// The approved client's identity, and the node's fingerprint, for the TLS flavor.
    client: Option<(Identity, cb2026_net::Fingerprint)>,
}

impl Node {
    fn start(flavor: Flavor) -> Node {
        Node::with(flavor, |_| {})
    }

    fn with(flavor: Flavor, adjust: impl FnOnce(&mut Config)) -> Node {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let mut config = Config::new("127.0.0.1:0".parse().unwrap(), "MIT OR Apache-2.0");
        let mut client = None;
        if let Flavor::Tls = flavor {
            let dir: PathBuf = std::env::temp_dir().join(format!(
                "cb2026-net-hard-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let trust = Arc::new(Trust::open(Store::new(dir)).unwrap());
            let me = Identity::generate().unwrap();
            trust
                .store()
                .approve(me.fingerprint(), "me", SystemTime::now())
                .unwrap();
            client = Some((me, trust.fingerprint()));
            config.trust = Some(trust);
        }
        adjust(&mut config);
        let server = Server::bind(config).expect("binds");
        let address = server.local_addr().unwrap();
        let stop = server.stop_handle();
        let thread = thread::spawn(move || server.serve(Engine::new).expect("serves"));
        Node {
            flavor,
            address,
            stop,
            thread: Some(thread),
            client,
        }
    }

    fn connect(&self) -> Wire {
        match &self.client {
            None => {
                let stream = TcpStream::connect(self.address).expect("connects");
                stream.set_read_timeout(Some(WAIT)).unwrap();
                Wire::Plain {
                    reader: BufReader::new(stream.try_clone().unwrap()),
                    stream,
                }
            }
            Some((me, pin)) => Wire::Tls(Box::new(
                client::connect(self.address, me, Some(*pin)).expect("connects"),
            )),
        }
    }

    /// Connects until served rather than refused, and initializes.
    fn connect_when_free(&self) -> Wire {
        let start = Instant::now();
        loop {
            let mut wire = self.connect();
            if wire.try_send(INITIALIZE) && wire.line().is_some_and(|l| l.contains(r#""result""#)) {
                return wire;
            }
            assert!(
                start.elapsed() < WAIT,
                "{:?}: the place never came free",
                self.flavor
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn finish(mut self) {
        self.stop.stop();
        self.thread.take().unwrap().join().expect("ends cleanly");
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// A client's end of a connection, of either flavor.
enum Wire {
    Plain {
        stream: TcpStream,
        reader: BufReader<TcpStream>,
    },
    Tls(Box<Connection>),
}

impl Wire {
    fn try_send(&mut self, line: &str) -> bool {
        self.try_bytes(format!("{line}\n").as_bytes())
    }

    fn send(&mut self, line: &str) {
        assert!(self.try_send(line), "the line couldn't be sent");
    }

    fn try_bytes(&mut self, bytes: &[u8]) -> bool {
        match self {
            Wire::Plain { stream, .. } => stream.write_all(bytes).is_ok(),
            Wire::Tls(c) => c.send_bytes(bytes).is_ok(),
        }
    }

    /// The next line, or `None` if the connection ended.
    fn line(&mut self) -> Option<String> {
        match self {
            Wire::Plain { reader, .. } => {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => None,
                    Ok(_) => Some(line),
                }
            }
            Wire::Tls(c) => c.read_line().ok().flatten(),
        }
    }

    /// Reads until a line containing `needle`.
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
        assert!(self.until(r#""id":1"#).contains(r#""result""#));
    }

    fn ping_works(&mut self, id: u32) -> bool {
        self.try_send(&format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#)) && {
            let needle = format!(r#""id":{id}"#);
            loop {
                match self.line() {
                    Some(line) if line.contains(&needle) => break line.contains("result"),
                    Some(_) => {}
                    None => break false,
                }
            }
        }
    }
}

/// Whether the other side is still holding this connection and saying nothing: a closed one
/// reads zero bytes or fails at once, and one that is being sent away (told `Busy`) reads that
/// answer; only a connection that is held open and silent times out.
fn is_open(stream: &TcpStream) -> bool {
    // A socket the other side has reset may refuse even to have its timeout set.
    if stream
        .set_read_timeout(Some(Duration::from_millis(5)))
        .is_err()
    {
        return false;
    }
    let mut byte = [0u8; 1];
    match (&*stream).read(&mut byte) {
        Ok(0) => false,
        Ok(_) => false,
        Err(e) => matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    }
}

#[test]
fn a_message_sent_in_pieces_is_put_back_together() {
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut wire = node.connect();
        let message = format!("{INITIALIZE}\n");
        // One or two bytes at a time, as a slow or badly buffered client might.
        for piece in message.as_bytes().chunks(2) {
            assert!(wire.try_bytes(piece));
        }
        assert!(
            wire.until(r#""id":1"#).contains(r#""result""#),
            "{flavor:?}"
        );
        assert!(wire.ping_works(2));
        node.finish();
    }
}

#[test]
fn malformed_messages_are_answered_with_errors_and_the_session_goes_on() {
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut wire = node.connect();
        wire.initialize();
        let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        let long_id = "9".repeat(10_000);
        for bad in [
            "not json".to_string(),
            "{}".to_string(),
            "[]".to_string(),
            "null".to_string(),
            "\"just a string\"".to_string(),
            r#"{"jsonrpc":"1.0","id":3,"method":"ping"}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":3,"method":"no/such/method"}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":3,"method":"engine/search"}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":3,"method":"engine/search","params":"x"}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":3,"method":"engine/search","params":{"position":7}}"#
                .to_string(),
            r#"{"jsonrpc":"2.0","id":{"a":1},"method":"ping"}"#.to_string(),
            format!(r#"{{"jsonrpc":"2.0","id":{long_id},"method":"ping"}}"#),
            deep,
            "\u{0}\u{0}\u{0}".to_string(),
        ] {
            assert!(wire.try_send(&bad), "{flavor:?}: sent {:.30}", bad);
            assert!(
                wire.ping_works(100),
                "{flavor:?}: the session ended after {:.40}",
                bad
            );
        }
        node.finish();
    }
}

#[test]
fn bytes_that_are_not_text_end_at_most_the_session() {
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut wire = node.connect();
        wire.initialize();
        wire.try_bytes(b"\xff\xfe\xfd\n");
        wire.try_bytes(&[0x80; 1000]);
        wire.try_bytes(b"\n");
        drop(wire);
        // Whatever it did to that session, the listener serves the next client.
        let mut next = node.connect_when_free();
        assert!(next.ping_works(5), "{flavor:?}");
        node.finish();
    }
}

#[test]
fn oversized_messages_are_refused_over_both_and_the_listener_stays_up() {
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut wire = node.connect();
        wire.initialize();
        // Past the limit with no newline: the node must answer and stop buffering.
        let huge = vec![b'a'; 2 * cb2026_net::MESSAGE_BYTES as usize];
        let _ = wire.try_bytes(&huge);
        let _ = wire.try_bytes(b"\n");
        let answer = wire.line().expect("the oversized message is answered");
        assert!(answer.contains(r#""error""#), "{flavor:?}: {answer}");
        drop(wire);
        node.connect_when_free();
        node.finish();
    }
}

#[test]
fn invalid_positions_moves_and_limits_get_answers_and_never_hurt_the_node() {
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut wire = node.connect();
        wire.initialize();
        let requests = [
            // (position JSON, limits JSON, must be an error)
            (r#"{"fen":""}"#, r#"{"depth":1}"#, true),
            (r#"{"fen":"not a fen"}"#, r#"{"depth":1}"#, true),
            (
                r#"{"fen":"8/8/8/8/8/8/8/8 w - - 0 1"}"#,
                r#"{"depth":1}"#,
                true,
            ),
            (
                r#"{"fen":"4k3/8/8/8/8/8/8/KK6 w - - 0 1"}"#,
                r#"{"depth":1}"#,
                true,
            ),
            (
                r#"{"fen":"P3k3/8/8/8/8/8/8/4K3 w - - 0 1"}"#,
                r#"{"depth":1}"#,
                true,
            ),
            (
                r#"{"fen":"4k2Q/8/8/8/8/8/8/4K3 w - - 0 1"}"#,
                r#"{"depth":1}"#,
                true,
            ),
            (
                r#"{"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1","moves":["e2e5"]}"#,
                r#"{"depth":1}"#,
                true,
            ),
            (
                r#"{"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1","moves":["zzzz"]}"#,
                r#"{"depth":1}"#,
                true,
            ),
            (
                r#"{"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1","moves":["e2e4","e2e4"]}"#,
                r#"{"depth":1}"#,
                true,
            ),
            (
                r#"{"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}"#,
                r#"{"depth":-1}"#,
                true,
            ),
            (
                r#"{"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}"#,
                r#"{"depth":"deep"}"#,
                true,
            ),
            (
                r#"{"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}"#,
                r#"{"timeMs":99999999999999999999999}"#,
                true,
            ),
            // Not necessarily errors: a node may answer a nonsense limit with its own.
            (
                r#"{"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}"#,
                r#"{"depth":0}"#,
                false,
            ),
            (
                r#"{"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}"#,
                r#"{"nodes":0}"#,
                false,
            ),
        ];
        for (i, (position, limits, must_fail)) in requests.iter().enumerate() {
            let id = 10 + i as u32;
            wire.send(&format!(
                r#"{{"jsonrpc":"2.0","id":{id},"method":"engine/search","params":{{"position":{position},"limits":{limits}}}}}"#
            ));
            let answer = wire.until(&format!(r#""id":{id}"#));
            if *must_fail {
                assert!(
                    answer.contains(r#""error""#),
                    "{flavor:?}: {position} {limits}: {answer}"
                );
            }
        }
        // Options and ids that name nothing real.
        wire.send(&format!(
            r#"{{"jsonrpc":"2.0","id":50,"method":"engine/search","params":{{"position":{{"fen":"{START}"}},"limits":{{"depth":1}},"multipv":4000000000,"techniques":{{"no-such-technique":true}},"rootMoves":["a1a1"]}}}}"#
        ));
        wire.until(r#""id":50"#);
        // The node is as it was: a real search still answers.
        wire.send(&search(60, r#"{"depth":3}"#));
        assert!(wire.until(r#""id":60"#).contains("bestMove"), "{flavor:?}");
        node.finish();
    }
}

#[test]
fn a_cancel_then_an_immediate_new_search_never_leaks_the_old_ones_answer() {
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut wire = node.connect();
        wire.initialize();
        for round in 0..10u32 {
            let (old, new) = (100 + 2 * round, 101 + 2 * round);
            wire.send(&search(old, r#"{"infinite":true}"#));
            wire.send(&cancel(old));
            wire.send(&search(new, r#"{"depth":2}"#));
            // The cancelled search answers with an error, never a move; the new one answers.
            let first = wire.until(&format!(r#""id":{old}"#));
            assert!(
                !first.contains("bestMove"),
                "{flavor:?}: a cancelled search moved: {first}"
            );
            let second = wire.until(&format!(r#""id":{new}"#));
            assert!(second.contains("bestMove"), "{flavor:?}: {second}");
        }
        node.finish();
    }
}

#[test]
fn request_ids_belong_to_a_connection_and_nothing_stale_reaches_the_next() {
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut first = node.connect();
        first.initialize();
        first.send(&search(2, r#"{"infinite":true}"#));
        drop(first);
        // The same ids on a new connection: its answers are its own.
        let mut next = node.connect_when_free();
        next.send(&search(2, r#"{"depth":3}"#));
        let answer = next.until(r#""id":2"#);
        assert!(
            answer.contains("bestMove"),
            "{flavor:?}: the first line for id 2 was {answer}"
        );
        node.finish();
    }
}

#[test]
fn half_a_message_then_a_disconnect_frees_the_place() {
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut wire = node.connect();
        wire.initialize();
        wire.try_bytes(&INITIALIZE.as_bytes()[..40]);
        drop(wire);
        let mut next = node.connect_when_free();
        assert!(next.ping_works(3));
        node.finish();
    }
}

#[test]
fn a_connected_client_that_stalls_mid_message_keeps_its_place_by_design() {
    // There is no idle timeout (a client waiting for a long search is silent too), so an
    // approved client that stops half-way through a message holds the one place until it
    // disconnects or its connection dies (keepalive). Another approved client is told Busy.
    for flavor in BOTH {
        let node = Node::start(flavor);
        let mut stalled = node.connect();
        stalled.initialize();
        stalled.try_bytes(&INITIALIZE.as_bytes()[..40]);
        let mut other = node.connect();
        let line = other.line().expect("a refusal");
        assert!(line.contains(r#""error""#), "{flavor:?}: {line}");
        node.finish();
    }
}

#[test]
fn a_client_that_never_reads_cannot_keep_the_listener_from_stopping() {
    for flavor in BOTH {
        let node = Node::with(flavor, |c| c.write_timeout = Duration::from_millis(300));
        let mut wire = node.connect();
        wire.initialize();
        // A flood of requests whose answers are never read: the node's writes back up.
        let flood = thread::spawn(move || {
            let ping = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;
            for _ in 0..200_000 {
                if !wire.try_send(ping) {
                    break;
                }
            }
        });
        thread::sleep(Duration::from_millis(1500));
        let start = Instant::now();
        node.finish();
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{flavor:?}: stopping took {:?}",
            start.elapsed()
        );
        flood.join().unwrap();
    }
}

#[test]
fn a_flood_of_connections_is_bounded_and_does_not_hurt_a_session() {
    for flavor in BOTH {
        let node = Node::with(flavor, |c| c.handshake_timeout = Duration::from_secs(5));
        let mut wire = node.connect();
        wire.initialize();
        // 150 connections that say nothing.
        let stalled: Vec<TcpStream> = (0..150)
            .filter_map(|_| TcpStream::connect(node.address).ok())
            .collect();
        thread::sleep(Duration::from_millis(500));
        // All but a few were closed on at once: a closed connection reads zero bytes.
        let still_open = stalled.iter().filter(|s| is_open(s)).count();
        assert!(
            still_open <= cb2026_net::PER_ADDRESS,
            "{flavor:?}: {still_open} of 150 connections from one address were kept"
        );
        // The session in progress didn't notice.
        assert!(wire.ping_works(9), "{flavor:?}");
        node.finish();
    }
}

#[test]
fn an_address_that_keeps_failing_is_ignored_for_a_while() {
    // Only TLS has a handshake to fail; a plain connection is never a failed greeting.
    for flavor in [Flavor::Tls] {
        let node = Node::with(flavor, |c| c.handshake_timeout = Duration::from_millis(150));
        // Rounds of silent connections, each timing out: failed greetings from one address.
        for _ in 0..(cb2026_net::FAILURES as usize / cb2026_net::PER_ADDRESS + 2) {
            let round: Vec<_> = (0..cb2026_net::PER_ADDRESS)
                .filter_map(|_| TcpStream::connect(node.address).ok())
                .collect();
            thread::sleep(Duration::from_millis(400));
            drop(round);
        }
        // Now even a real client from that address is closed on unanswered: its connection is
        // reset, or ends with nothing said.
        let (me, pin) = node.client.as_ref().unwrap();
        match client::connect(node.address, me, Some(*pin)) {
            Err(_) => {}
            Ok(mut c) => {
                let sent = c.send_line(INITIALIZE).is_ok();
                assert!(
                    !sent || c.read_line().ok().flatten().is_none(),
                    "{flavor:?}: served during a ban"
                );
            }
        }
        node.finish();
    }
}

#[test]
fn a_client_without_a_certificate_is_not_served() {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme};

    #[derive(Debug)]
    struct Anything;
    impl ServerCertVerifier for Anything {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    let node = Node::start(Flavor::Tls);
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Anything))
            .with_no_client_auth();
    let mut socket = TcpStream::connect(node.address).unwrap();
    socket.set_read_timeout(Some(WAIT)).unwrap();
    let mut tls =
        ClientConnection::new(Arc::new(config), ServerName::try_from("cb2026").unwrap()).unwrap();
    let mut served = false;
    let handshake: std::io::Result<()> = (|| {
        while tls.is_handshaking() {
            tls.complete_io(&mut socket)?;
        }
        let mut stream = rustls::Stream::new(&mut tls, &mut socket);
        stream.write_all(format!("{INITIALIZE}\n").as_bytes())?;
        let mut byte = [0u8; 1];
        served = stream.read(&mut byte)? > 0;
        Ok(())
    })();
    assert!(
        !served,
        "a client with no certificate was served ({handshake:?})"
    );
    // Nothing broke.
    node.connect_when_free();
    node.finish();
}

#[test]
fn a_plain_listener_is_never_offered_beyond_loopback() {
    let config = Config::new("0.0.0.0:0".parse().unwrap(), "MIT OR Apache-2.0");
    assert!(Server::bind(config).is_err());
    let config = Config::new("[::]:0".parse().unwrap(), "MIT OR Apache-2.0");
    assert!(Server::bind(config).is_err());
}
