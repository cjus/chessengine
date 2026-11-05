//! End-to-end tests of the listener with trust: TLS, pairing and the approved-clients list, over
//! real loopback connections. A `cb2026_net::client::Connection` plays the other computer.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use cb2026_engine::Engine;
use cb2026_net::client::{self, Connection};
use cb2026_net::{Config, Fingerprint, Identity, Server, StopHandle, Store, Trust};

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":"1.0","client":{"name":"test","version":"0"}}}"#;
const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const WAIT: Duration = Duration::from_secs(30);

fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "cb2026-net-tls-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A listener with trust, running on a thread of its own.
struct Node {
    address: SocketAddr,
    trust: Arc<Trust>,
    stop: StopHandle,
    thread: Option<JoinHandle<()>>,
}

impl Node {
    fn start() -> Node {
        Node::with(|_| {})
    }

    fn with(adjust: impl FnOnce(&mut Config)) -> Node {
        let trust = Arc::new(Trust::open(Store::new(temp_dir("node"))).expect("trust opens"));
        let mut config = Config::new("127.0.0.1:0".parse().unwrap(), "MIT OR Apache-2.0");
        config.trust = Some(Arc::clone(&trust));
        adjust(&mut config);
        let server = Server::bind(config).expect("binds");
        let address = server.local_addr().unwrap();
        let stop = server.stop_handle();
        let thread = thread::spawn(move || server.serve(Engine::new).expect("serves"));
        Node {
            address,
            trust,
            stop,
            thread: Some(thread),
        }
    }

    fn fingerprint(&self) -> Fingerprint {
        self.trust.fingerprint()
    }

    /// A client the person has already approved.
    fn approved_client(&self, name: &str) -> Identity {
        let identity = Identity::generate().unwrap();
        self.trust
            .store()
            .approve(identity.fingerprint(), name, SystemTime::now())
            .unwrap();
        identity
    }

    fn connect(&self, identity: &Identity) -> Connection {
        client::connect(self.address, identity, Some(self.fingerprint())).expect("connects")
    }

    /// The code of a newly opened pairing window.
    fn open_window(&self) -> String {
        self.trust
            .store()
            .open_pairing(SystemTime::now())
            .unwrap()
            .code
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

fn initialize(connection: &mut Connection) {
    connection.send_line(INITIALIZE).expect("sends");
    let answer = connection.read_line().expect("reads").expect("an answer");
    assert!(answer.contains(r#""result""#), "{answer}");
}

/// Whether the node closes the connection without saying anything: what a stranger meets.
fn is_closed_silently(connection: &mut Connection) -> bool {
    matches!(connection.read_line(), Ok(None) | Err(_))
}

#[test]
fn an_approved_client_searches_over_tls_and_says_goodbye() {
    let node = Node::start();
    let me = node.approved_client("my laptop");
    let mut connection = node.connect(&me);
    initialize(&mut connection);
    connection
        .send_line(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"engine/search","params":{{"position":{{"fen":"{START}"}},"limits":{{"depth":3}}}}}}"#
        ))
        .unwrap();
    loop {
        let line = connection.read_line().unwrap().expect("a line");
        if line.contains(r#""id":2"#) {
            assert!(line.contains("bestMove"), "{line}");
            break;
        }
    }
    connection
        .send_line(r#"{"jsonrpc":"2.0","id":99,"method":"shutdown"}"#)
        .unwrap();
    connection.read_line().unwrap();
    connection
        .send_line(r#"{"jsonrpc":"2.0","method":"exit"}"#)
        .unwrap();
    assert!(is_closed_silently(&mut connection));
    node.finish();
}

#[test]
fn pairing_with_the_code_approves_the_client_for_good() {
    let node = Node::start();
    let me = Identity::generate().unwrap();
    let code = node.open_window();

    // 1. Connect without knowing the node, and read the fingerprint to show the person.
    let mut first = client::connect(node.address, &me, None).expect("connects");
    assert_eq!(first.server_fingerprint(), node.fingerprint());
    // 2. The person says it matches, so the code goes. (Spaces in a typed code don't matter.)
    let typed = format!("{} {}", &code[..4], &code[4..]);
    assert!(first.pair(&typed, "Carlos's iMac").unwrap());

    // 3. From now on the client connects pinned, with no code, and is served.
    let mut again = node.connect(&me);
    initialize(&mut again);
    let approved = node.trust.store().approved().unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].fingerprint, me.fingerprint());
    assert_eq!(approved[0].name, "Carlos's iMac");
    node.finish();
}

#[test]
fn a_code_lets_in_one_client_and_then_the_window_is_closed() {
    let node = Node::start();
    let (one, two) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let code = node.open_window();
    assert!(
        client::connect(node.address, &one, None)
            .unwrap()
            .pair(&code, "one")
            .unwrap()
    );
    let mut second = client::connect(node.address, &two, None).unwrap();
    assert!(!second.pair(&code, "two").unwrap(), "the code is spent");
    assert!(!node.trust.store().is_approved(&two.fingerprint()));
    node.finish();
}

#[test]
fn wrong_codes_say_nothing_and_three_close_the_window() {
    let node = Node::start();
    let me = Identity::generate().unwrap();
    let code = node.open_window();
    let wrong = if code == "00000000" {
        "11111111"
    } else {
        "00000000"
    };
    for _ in 0..cb2026_net::trust::PAIRING_ATTEMPTS {
        let mut c = client::connect(node.address, &me, None).unwrap();
        assert!(!c.pair(wrong, "me").unwrap());
    }
    // Even the right code is too late now.
    let mut c = client::connect(node.address, &me, None).unwrap();
    assert!(!c.pair(&code, "me").unwrap());
    assert!(!node.trust.store().is_approved(&me.fingerprint()));
    node.finish();
}

#[test]
fn a_stranger_with_no_pairing_window_is_closed_on_in_silence() {
    let node = Node::start();
    let stranger = Identity::generate().unwrap();
    let mut c = client::connect(node.address, &stranger, Some(node.fingerprint())).unwrap();
    c.send_line(INITIALIZE).ok();
    assert!(
        is_closed_silently(&mut c),
        "nothing is said, not even an error"
    );
    // A pairing attempt with no window open says nothing either.
    let mut c = client::connect(node.address, &stranger, None).unwrap();
    assert!(!c.pair("12345678", "x").unwrap());
    node.finish();
}

#[test]
fn a_stranger_cannot_use_cnp_during_a_pairing_window() {
    let node = Node::start();
    node.open_window();
    let stranger = Identity::generate().unwrap();
    let mut c = client::connect(node.address, &stranger, None).unwrap();
    c.send_line(INITIALIZE).ok();
    assert!(is_closed_silently(&mut c));
    node.finish();
}

#[test]
fn a_client_refuses_a_node_whose_fingerprint_it_does_not_know() {
    let node = Node::start();
    let me = node.approved_client("me");
    let someone_else = Identity::generate().unwrap().fingerprint();
    let result = client::connect(node.address, &me, Some(someone_else));
    assert!(
        result.is_err(),
        "a node with another certificate is refused"
    );
    node.finish();
}

#[test]
fn revoking_a_client_refuses_it_at_its_next_connection() {
    let node = Node::start();
    let me = node.approved_client("me");
    let mut connection = node.connect(&me);
    initialize(&mut connection);
    connection
        .send_line(r#"{"jsonrpc":"2.0","method":"exit"}"#)
        .unwrap();
    assert!(is_closed_silently(&mut connection));

    assert!(node.trust.store().revoke(&me.fingerprint()).unwrap());
    let mut again = node.connect(&me);
    again.send_line(INITIALIZE).ok();
    assert!(is_closed_silently(&mut again), "no restart was needed");
    node.finish();
}

#[test]
fn a_stolen_certificate_without_its_key_gets_nowhere() {
    use rustls::client::ResolvesClientCert;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
    use rustls::sign::CertifiedKey;
    use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme};

    // The approved client's certificate is public (it crosses the wire in every handshake);
    // an attacker presents it with a key of their own.
    let node = Node::start();
    let genuine = rcgen::generate_simple_self_signed(vec!["cb2026".to_string()]).unwrap();
    node.trust
        .store()
        .approve(
            Fingerprint::of(genuine.cert.der()),
            "genuine",
            SystemTime::now(),
        )
        .unwrap();
    let attacker_key = rcgen::KeyPair::generate().unwrap();

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

    #[derive(Debug)]
    struct Mismatched(Arc<CertifiedKey>);
    impl ResolvesClientCert for Mismatched {
        fn resolve(&self, _: &[&[u8]], _: &[SignatureScheme]) -> Option<Arc<CertifiedKey>> {
            Some(Arc::clone(&self.0))
        }
        fn has_certs(&self) -> bool {
            true
        }
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let signer = provider
        .key_provider
        .load_private_key(PrivatePkcs8KeyDer::from(attacker_key.serialize_der()).into())
        .unwrap();
    let forged = Arc::new(CertifiedKey::new(vec![genuine.cert.der().clone()], signer));
    let config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Anything))
        .with_client_cert_resolver(Arc::new(Mismatched(forged)));

    let mut socket = TcpStream::connect(node.address).unwrap();
    socket.set_read_timeout(Some(WAIT)).unwrap();
    let mut tls =
        ClientConnection::new(Arc::new(config), ServerName::try_from("cb2026").unwrap()).unwrap();
    // The handshake fails on the node's side: its alert, or the end of the connection, reaches
    // this client before any protocol could be spoken.
    let mut outcome = Ok(());
    while tls.is_handshaking() {
        if let Err(e) = tls.complete_io(&mut socket) {
            outcome = Err(e);
            break;
        }
    }
    let spoke = outcome.is_ok() && {
        let mut stream = rustls::Stream::new(&mut tls, &mut socket);
        stream.write_all(INITIALIZE.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok() && {
            let mut buffer = [0u8; 1];
            matches!(stream.read(&mut buffer), Ok(n) if n > 0)
        }
    };
    assert!(!spoke, "a certificate without its key was served");
    // The node is fine, and the genuine owner of the key would be.
    node.finish();
}

#[test]
fn a_second_approved_client_is_told_busy_and_the_first_is_not_disturbed() {
    let node = Node::start();
    let (a, b) = (node.approved_client("a"), node.approved_client("b"));
    let mut first = node.connect(&a);
    initialize(&mut first);
    let mut second = node.connect(&b);
    let line = second.read_line().unwrap().expect("a refusal is sent");
    assert!(
        line.contains(r#""id":null"#) && line.contains(r#""error""#),
        "{line}"
    );
    assert!(is_closed_silently(&mut second));
    first
        .send_line(r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#)
        .unwrap();
    assert!(first.read_line().unwrap().unwrap().contains(r#""id":5"#));
    node.finish();
}

#[test]
fn a_stranger_holding_a_connection_does_not_keep_an_approved_client_out() {
    let node = Node::start();
    // Strangers that finish the handshake and then say nothing.
    // Fewer than one address may have greeted at once (see `abuse`), as all of these share one.
    let idle: Vec<_> = (0..cb2026_net::PER_ADDRESS - 1)
        .map(|_| {
            let id = Identity::generate().unwrap();
            client::connect(node.address, &id, None).unwrap()
        })
        .collect();
    let me = node.approved_client("me");
    let mut connection = node.connect(&me);
    initialize(&mut connection);
    drop(idle);
    node.finish();
}

#[test]
fn a_disconnect_over_tls_cancels_the_search_and_frees_the_place() {
    let node = Node::start();
    let me = node.approved_client("me");
    let mut connection = node.connect(&me);
    initialize(&mut connection);
    connection
        .send_line(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"engine/search","params":{{"position":{{"fen":"{START}"}},"limits":{{"infinite":true}}}}}}"#
        ))
        .unwrap();
    thread::sleep(Duration::from_millis(300));
    drop(connection);
    // The place comes free: the same client is served again, not told busy.
    let start = Instant::now();
    loop {
        let mut next = node.connect(&me);
        next.send_line(INITIALIZE).unwrap();
        match next.read_line() {
            Ok(Some(line)) if line.contains(r#""result""#) => break,
            _ => assert!(start.elapsed() < WAIT, "the place never came free"),
        }
        thread::sleep(Duration::from_millis(20));
    }
    node.finish();
}

#[test]
fn a_silent_connection_is_dropped_at_the_deadline_and_a_trickle_does_not_renew_it() {
    let node = Node::with(|c| c.handshake_timeout = Duration::from_millis(500));
    // Says nothing at all.
    let mut silent = TcpStream::connect(node.address).unwrap();
    silent.set_read_timeout(Some(WAIT)).unwrap();
    let start = Instant::now();
    let mut buffer = [0u8; 16];
    assert!(matches!(silent.read(&mut buffer), Ok(0) | Err(_)));
    assert!(start.elapsed() < Duration::from_secs(10));

    // Starts a TLS record that says it is 5000 bytes long and sends a byte of it every 100 ms:
    // each read is quick, but the handshake never ends.
    let mut slow = TcpStream::connect(node.address).unwrap();
    slow.set_read_timeout(Some(WAIT)).unwrap();
    slow.write_all(&[0x16, 0x03, 0x01, 0x13, 0x88]).unwrap();
    let start = Instant::now();
    for _ in 0..40 {
        if slow.write_all(&[0x01]).is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
        if start.elapsed() > Duration::from_secs(3) {
            break;
        }
    }
    // Whatever the node sent on the way out (an alert), the connection then ends.
    while matches!(slow.read(&mut buffer), Ok(n) if n > 0) {}
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "the deadline was renewed by the trickle: {:?}",
        start.elapsed()
    );
    node.finish();
}

#[test]
fn plaintext_and_garbage_are_refused_and_the_node_carries_on() {
    let node = Node::start();
    let mut plain = TcpStream::connect(node.address).unwrap();
    plain.set_read_timeout(Some(WAIT)).unwrap();
    plain.write_all(INITIALIZE.as_bytes()).unwrap();
    plain.write_all(b"\n").unwrap();
    let mut buffer = [0u8; 4096];
    // Any answer is a TLS alert (starting 0x15), never a CNP reply.
    if let Ok(n) = plain.read(&mut buffer) {
        assert!(n == 0 || buffer[0] == 0x15, "{:?}", &buffer[..n.min(40)]);
    }
    let mut noise = TcpStream::connect(node.address).unwrap();
    noise.write_all(&[0xFF; 70_000]).ok();
    let me = node.approved_client("me");
    let mut connection = node.connect(&me);
    initialize(&mut connection);
    node.finish();
}

#[test]
fn an_oversized_or_malformed_pairing_request_is_closed_on() {
    let node = Node::start();
    let code = node.open_window();
    let me = Identity::generate().unwrap();
    for request in [
        "x".repeat(5_000),
        "not json".to_string(),
        r#"{"cb2026":"other","code":"1"}"#.to_string(),
        r#"{"cb2026":"pair"}"#.to_string(),
        format!(r#"{{"cb2026":"pair","code":{code}}}"#), // a number, not a string
    ] {
        let mut c = client::connect(node.address, &me, None).unwrap();
        c.send_line(&request).ok();
        assert!(is_closed_silently(&mut c), "{:.40}", request);
    }
    // None of that spent the window's guesses on a real code, and the right code still works.
    let mut c = client::connect(node.address, &me, None).unwrap();
    assert!(c.pair(&code, "me").unwrap());
    node.finish();
}

#[test]
fn a_listener_with_trust_may_bind_beyond_loopback_and_one_without_may_not() {
    let trust = Arc::new(Trust::open(Store::new(temp_dir("bind"))).unwrap());
    let mut config = Config::new("0.0.0.0:0".parse().unwrap(), "MIT OR Apache-2.0");
    assert!(
        Server::bind(config.clone()).is_err(),
        "plaintext beyond loopback"
    );
    config.trust = Some(trust);
    assert!(Server::bind(config).is_ok());
}

#[test]
fn stopping_ends_a_tls_session_and_a_connection_still_in_its_handshake() {
    let node = Node::start();
    let me = node.approved_client("me");
    let mut connection = node.connect(&me);
    initialize(&mut connection);
    let _half_open = TcpStream::connect(node.address).unwrap();
    let start = Instant::now();
    let stop = node.stop.clone();
    stop.stop();
    assert!(is_closed_silently(&mut connection));
    node.finish();
    assert!(start.elapsed() < Duration::from_secs(10));
}
