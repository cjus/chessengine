//! End-to-end tests of discovery: a client's probe and an engine's answer over real UDP, on
//! loopback (a broadcast can't be tested portably; the messages and rules are the same).

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use cb2026_engine::Engine;
use cb2026_net::client::{self, Found};
use cb2026_net::discovery::{self, PER_SENDER, PROBE_BYTES};
use cb2026_net::{Config, Discovery, Error, Identity, Server, StopHandle, Store, Trust};

fn temp_dir() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!(
        "cb2026-net-disc-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

struct Node {
    probe_at: SocketAddr,
    tcp: SocketAddr,
    trust: Arc<Trust>,
    stop: StopHandle,
    thread: Option<JoinHandle<()>>,
}

impl Node {
    fn start(name: &str) -> Node {
        let trust = Arc::new(Trust::open(Store::new(temp_dir())).unwrap());
        let mut config = Config::new("127.0.0.1:0".parse().unwrap(), "MIT OR Apache-2.0");
        config.trust = Some(Arc::clone(&trust));
        config.discovery = Some(Discovery {
            port: 0,
            name: name.to_string(),
        });
        let server = Server::bind(config).expect("binds");
        let probe_at =
            SocketAddr::from((Ipv4Addr::LOCALHOST, server.discovery_addr().unwrap().port()));
        let tcp = server.local_addr().unwrap();
        let stop = server.stop_handle();
        let thread = thread::spawn(move || server.serve(Engine::new).expect("serves"));
        Node {
            probe_at,
            tcp,
            trust,
            stop,
            thread: Some(thread),
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

fn find(node: &Node) -> Vec<Found> {
    client::discover_at(&[node.probe_at], Duration::from_millis(600)).unwrap()
}

/// Sends raw datagrams to the node and counts what comes back in `wait`.
fn replies_to(node: &Node, datagrams: &[Vec<u8>], wait: Duration) -> Vec<Vec<u8>> {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.set_read_timeout(Some(wait)).unwrap();
    for d in datagrams {
        socket.send_to(d, node.probe_at).unwrap();
    }
    let mut got = Vec::new();
    let mut buffer = [0u8; 2048];
    while let Ok(n) = socket.recv(&mut buffer) {
        got.push(buffer[..n].to_vec());
    }
    got
}

#[test]
fn a_probe_is_answered_with_what_a_client_needs_to_pair() {
    let node = Node::start("the study");
    let found = find(&node);
    assert_eq!(found.len(), 1, "{found:?}");
    let a = &found[0].announcement;
    assert_eq!(a.name, "the study");
    assert_eq!(a.protocol, "1.0");
    assert_eq!(a.fingerprint, node.trust.fingerprint());
    assert!(!a.pairing, "no window is open");
    // The client connects to the address the reply came from, at the port it named.
    assert_eq!(found[0].address, node.tcp);

    // Opening a pairing window shows in the next answer.
    node.trust.store().open_pairing(SystemTime::now()).unwrap();
    assert!(find(&node)[0].announcement.pairing);
    node.finish();
}

#[test]
fn a_discovered_engine_can_be_paired_with_and_used() {
    let node = Node::start("pairable");
    let code = node
        .trust
        .store()
        .open_pairing(SystemTime::now())
        .unwrap()
        .code;
    let found = &find(&node)[0];
    let me = Identity::generate().unwrap();
    // The reply's fingerprint is a claim; connecting without a pin shows what the node proves.
    let mut first = client::connect(found.address, &me, None).unwrap();
    assert_eq!(first.server_fingerprint(), found.announcement.fingerprint);
    assert!(first.pair(&code, "found by discovery").unwrap());
    let mut again =
        client::connect(found.address, &me, Some(found.announcement.fingerprint)).unwrap();
    again
        .send_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":"1.0","client":{"name":"t","version":"0"}}}"#)
        .unwrap();
    assert!(again.read_line().unwrap().unwrap().contains(r#""result""#));
    node.finish();
}

#[test]
fn two_engines_are_both_found_and_told_apart() {
    let (one, two) = (Node::start("one"), Node::start("two"));
    let found =
        client::discover_at(&[one.probe_at, two.probe_at], Duration::from_millis(600)).unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
    let mut names: Vec<_> = found.iter().map(|f| f.announcement.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["one", "two"]);
    assert_ne!(
        found[0].announcement.fingerprint,
        found[1].announcement.fingerprint
    );
    assert_ne!(found[0].address, found[1].address);
    one.finish();
    two.finish();
}

#[test]
fn a_datagram_that_is_not_exactly_a_probe_gets_no_answer() {
    let node = Node::start("quiet");
    let good = discovery::probe();
    let mut long = good.clone();
    long.push(b' ');
    let mut other = br#"{"cnp":"here","v":1}"#.to_vec();
    other.resize(PROBE_BYTES, b' ');
    let bad = vec![
        good[..PROBE_BYTES - 1].to_vec(),
        long,
        b"{}".to_vec(),
        vec![],
        vec![0xFF; PROBE_BYTES],
        vec![b'a'; 8_000],
        other,
    ];
    assert!(replies_to(&node, &bad, Duration::from_millis(400)).is_empty());
    // Still alive, and a real probe is answered.
    assert_eq!(
        replies_to(&node, &[good], Duration::from_millis(400)).len(),
        1
    );
    node.finish();
}

#[test]
fn no_reply_is_bigger_than_the_probe_that_asked() {
    let node = Node::start(&"😀".repeat(40));
    node.trust.store().open_pairing(SystemTime::now()).unwrap();
    let got = replies_to(&node, &[discovery::probe()], Duration::from_millis(400));
    assert_eq!(got.len(), 1);
    assert!(got[0].len() <= PROBE_BYTES, "{} bytes", got[0].len());
    node.finish();
}

#[test]
fn a_stream_of_probes_gets_a_trickle_of_replies() {
    let node = Node::start("busy");
    let probes = vec![discovery::probe(); 200];
    let got = replies_to(&node, &probes, Duration::from_millis(500));
    // One second's allowance, with room for a second boundary falling inside the burst.
    assert!(!got.is_empty());
    assert!(
        got.len() <= 2 * PER_SENDER as usize,
        "{} replies to 200 probes",
        got.len()
    );
    node.finish();
}

#[test]
fn the_reply_says_nothing_private() {
    let node = Node::start("plain name");
    let got = replies_to(&node, &[discovery::probe()], Duration::from_millis(400));
    let text = String::from_utf8(got[0].clone()).unwrap();
    // Only the documented fields, and nothing from the machine or its owner.
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "cnp",
            "fingerprint",
            "name",
            "pairing",
            "port",
            "protocol",
            "v"
        ]
    );
    node.finish();
}

#[test]
fn discovery_needs_the_encrypted_listener() {
    let mut config = Config::new("127.0.0.1:0".parse().unwrap(), "MIT OR Apache-2.0");
    config.discovery = Some(Discovery {
        port: 0,
        name: "x".into(),
    });
    assert!(matches!(
        Server::bind(config),
        Err(Error::DiscoveryWithoutTrust)
    ));
}

#[test]
fn a_port_already_in_use_is_reported_before_serving() {
    let taken = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    let trust = Arc::new(Trust::open(Store::new(temp_dir())).unwrap());
    let mut config = Config::new("127.0.0.1:0".parse().unwrap(), "MIT OR Apache-2.0");
    config.trust = Some(trust);
    config.discovery = Some(Discovery {
        port: taken.local_addr().unwrap().port(),
        name: "x".into(),
    });
    assert!(matches!(Server::bind(config), Err(Error::Io(_))));
}

#[test]
fn stopping_ends_the_responder_and_closes_its_port() {
    let node = Node::start("brief");
    let at = node.probe_at;
    node.finish();
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    socket.send_to(&discovery::probe(), at).unwrap();
    assert!(
        socket.recv(&mut [0u8; 512]).is_err(),
        "nothing answers after the stop"
    );
}
