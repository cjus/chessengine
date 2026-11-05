//! Finding an engine on the local network: a UDP probe and a reply.
//!
//! A person who is less technical should never have to find an IP address, so a client can ask
//! the network *"is there a cb2026 here?"* and list the answers. This module is both halves of
//! that exchange: what the engine answers with (a responder thread, opt-in with `--discover`), and the
//! messages and rules a client follows ([`probe`], [`Announcement::parse`]; the client's search
//! itself is `crate::client::discover`).
//!
//! ## The messages
//!
//! - **Probe:** a datagram of exactly [`PROBE_BYTES`] bytes: the JSON `{"cnp":"probe","v":1}`
//!   padded with spaces. Anything else, in size or content, is ignored without a reply.
//! - **Reply:** one JSON line, sent to whoever sent the probe: `{"cnp":"here","v":1,
//!   "name":…,"protocol":"1.0","port":4026,"fingerprint":"AB:CD:…","pairing":false}`: the name
//!   its owner gave the node (`--name`), the CNP version, the TCP port, the certificate
//!   fingerprint, and whether a pairing window is open.
//!
//! ## What keeps it from being a weapon or a leak
//!
//! - **No amplification.** A UDP source address can be forged, so a reply must never be bigger
//!   than the probe that asked for it: the probe is padded to a fixed size, the reply is checked
//!   against it (tested with the longest name), and a datagram of any other size gets nothing.
//! - **Rate limited.** At most [`PER_SENDER`] replies a second to one address, and [`OVERALL`] in
//!   all, so even a fast stream of probes makes a trickle of replies, and the table that counts
//!   senders is bounded.
//! - **Nothing private.** No hostname, user name, path, CPU or memory: the name is one the owner
//!   chose, and the fingerprint is in every TLS handshake anyway.
//! - **A hint, not a credential.** A reply says where an engine *claims* to be. A client connects
//!   there and checks the fingerprint the way it always does (pinned, or confirmed by the person
//!   when pairing), so a forged reply can only send it to a machine that then fails the check.
//! - **Opt-in twice.** Only a listener with TLS (so one that may be reached from other computers)
//!   can answer, and only when started with `--discover`. No flag, no socket, no answers.
//!
//! ## Where it works, and where it doesn't
//!
//! A broadcast reaches the local network segment only. It doesn't cross a router or a VLAN, many
//! Wi-Fi access points keep clients from seeing each other, and a firewall can block the UDP
//! port; a client must always offer typing an address. IPv6 has no broadcast, so this is IPv4;
//! a multicast group could carry the same messages. Sending to `255.255.255.255` reaches only one
//! interface on some systems, so a client sends to each interface's own broadcast address too.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};

use crate::Trust;
use crate::identity::Fingerprint;

/// The UDP port engines listen on for probes, and clients send them to. The same number as the
/// default TCP port, which is a different protocol and so doesn't clash.
pub const PORT: u16 = 4026;

/// The size of every probe, and the most a reply may be.
pub const PROBE_BYTES: usize = 320;

/// Replies to one sender in a second, at most.
pub const PER_SENDER: u32 = 4;

/// Replies in a second, at most, to everyone.
pub const OVERALL: u32 = 40;

/// The longest node name, in bytes.
pub const NAME_BYTES: usize = 32;

/// How many senders the limiter remembers at once.
const TRACKED: usize = 1024;

/// How often the responder looks for a stop between datagrams.
const POLL: Duration = Duration::from_millis(100);

/// The version of the probe and reply messages.
const VERSION: u64 = 1;

/// The probe datagram: its JSON, padded to [`PROBE_BYTES`].
pub fn probe() -> Vec<u8> {
    let mut datagram = json!({ "cnp": "probe", "v": VERSION })
        .to_string()
        .into_bytes();
    datagram.resize(PROBE_BYTES, b' ');
    datagram
}

/// Whether a datagram is a probe.
fn is_probe(datagram: &[u8]) -> bool {
    datagram.len() == PROBE_BYTES
        && serde_json::from_slice::<Value>(datagram)
            .is_ok_and(|v| v["cnp"] == "probe" && v["v"] == VERSION)
}

/// What an engine says about itself in a reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    /// The name its owner gave it.
    pub name: String,
    /// The Chess Node Protocol version it speaks, such as `1.0`.
    pub protocol: String,
    /// The TCP port to connect to, on the address the reply came from.
    pub port: u16,
    /// The fingerprint of its certificate.
    pub fingerprint: Fingerprint,
    /// Whether a pairing window is open, so a new client can pair now.
    pub pairing: bool,
}

impl Announcement {
    /// The reply datagram, or `None` if it would be bigger than a probe (which the limits on
    /// the name make impossible).
    fn encode(&self) -> Option<Vec<u8>> {
        let reply = json!({
            "cnp": "here",
            "v": VERSION,
            "name": self.name,
            "protocol": self.protocol,
            "port": self.port,
            "fingerprint": self.fingerprint.to_string(),
            "pairing": self.pairing,
        })
        .to_string()
        .into_bytes();
        (reply.len() <= PROBE_BYTES).then_some(reply)
    }

    /// Reads a reply a client received, or `None` if it isn't a well-formed one. A client
    /// ignores such datagrams: anyone on the network can send it anything.
    pub fn parse(datagram: &[u8]) -> Option<Announcement> {
        if datagram.len() > PROBE_BYTES {
            return None;
        }
        let v: Value = serde_json::from_slice(datagram).ok()?;
        if v["cnp"] != "here" || v["v"] != VERSION {
            return None;
        }
        let name = v["name"].as_str()?;
        if name.len() > NAME_BYTES {
            return None;
        }
        Some(Announcement {
            name: name.to_string(),
            protocol: v["protocol"].as_str()?.to_string(),
            port: u16::try_from(v["port"].as_u64()?)
                .ok()
                .filter(|p| *p != 0)?,
            fingerprint: v["fingerprint"].as_str()?.parse().ok()?,
            pairing: v["pairing"].as_bool()?,
        })
    }
}

/// A name fit to put in a reply: no control characters, quotes or backslashes, trimmed, and at
/// most [`NAME_BYTES`] bytes (cut at a character). Empty becomes `cb2026`.
pub fn clean_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
        .collect();
    let mut cut = String::new();
    for c in kept
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
    {
        if cut.len() + c.len_utf8() > NAME_BYTES {
            break;
        }
        cut.push(c);
    }
    if cut.is_empty() {
        "cb2026".to_string()
    } else {
        cut
    }
}

/// How many replies each sender, and everyone, may have in the second now running.
#[derive(Debug)]
struct RateLimit {
    started: Instant,
    overall: u32,
    senders: HashMap<IpAddr, u32>,
}

impl RateLimit {
    fn new(now: Instant) -> RateLimit {
        RateLimit {
            started: now,
            overall: 0,
            senders: HashMap::new(),
        }
    }

    /// Whether `sender` may be answered at `now`, counting it if so.
    fn allow(&mut self, sender: IpAddr, now: Instant) -> bool {
        if now.duration_since(self.started) >= Duration::from_secs(1) {
            *self = RateLimit::new(now);
        }
        if self.overall >= OVERALL {
            return false;
        }
        // A flood from many addresses can't grow the table without end: past the bound, new
        // senders are not answered (the second's window empties it soon enough).
        if !self.senders.contains_key(&sender) && self.senders.len() >= TRACKED {
            return false;
        }
        let count = self.senders.entry(sender).or_insert(0);
        if *count >= PER_SENDER {
            return false;
        }
        *count += 1;
        self.overall += 1;
        true
    }
}

/// Answers probes on a UDP socket, on a thread of its own, until told to stop.
pub(crate) struct Responder {
    handle: JoinHandle<()>,
}

/// The facts a reply is made from, besides whether pairing is open.
pub(crate) struct Who {
    pub(crate) name: String,
    pub(crate) tcp_port: u16,
    pub(crate) trust: Arc<Trust>,
}

/// Opens the probe socket on all of this computer's IPv4 addresses.
pub(crate) fn bind(port: u16) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    socket.set_read_timeout(Some(POLL))?;
    Ok(socket)
}

impl Responder {
    pub(crate) fn start(socket: UdpSocket, who: Who, stop: Arc<AtomicBool>) -> Responder {
        let handle = thread::spawn(move || respond(&socket, &who, &stop));
        Responder { handle }
    }

    pub(crate) fn join(self) {
        let _ = self.handle.join();
    }
}

fn respond(socket: &UdpSocket, who: &Who, stop: &AtomicBool) {
    let mut limit = RateLimit::new(Instant::now());
    // One byte more than a probe, so a datagram that is too long is seen as too long.
    let mut buffer = [0u8; PROBE_BYTES + 1];
    while !stop.load(Ordering::Relaxed) {
        let (n, sender) = match socket.recv_from(&mut buffer) {
            Ok(received) => received,
            // A timeout is the poll; anything else (an ICMP error from an earlier reply, say)
            // concerns one datagram, not the socket.
            Err(_) => continue,
        };
        if !is_probe(&buffer[..n]) || !limit.allow(sender.ip(), Instant::now()) {
            continue;
        }
        let announcement = Announcement {
            name: who.name.clone(),
            protocol: protocol(),
            port: who.tcp_port,
            fingerprint: who.trust.fingerprint(),
            pairing: who.trust.store().pairing_is_open(SystemTime::now()),
        };
        if let Some(reply) = announcement.encode() {
            let _ = socket.send_to(&reply, SocketAddr::new(sender.ip(), sender.port()));
        }
    }
}

/// The CNP version this node speaks, as `major.minor`.
fn protocol() -> String {
    let v = cnp::lifecycle::PROTOCOL_VERSION;
    format!("{}.{}", v.major, v.minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint() -> Fingerprint {
        crate::Identity::generate().unwrap().fingerprint()
    }

    fn announcement(name: &str) -> Announcement {
        Announcement {
            name: name.to_string(),
            protocol: "1.0".to_string(),
            port: 65_535,
            fingerprint: fingerprint(),
            pairing: false,
        }
    }

    #[test]
    fn a_probe_is_the_fixed_size_and_is_recognised() {
        let datagram = probe();
        assert_eq!(datagram.len(), PROBE_BYTES);
        assert!(is_probe(&datagram));
    }

    #[test]
    fn only_an_exact_probe_is_a_probe() {
        let good = probe();
        assert!(!is_probe(&good[..PROBE_BYTES - 1]), "too short");
        let mut long = good.clone();
        long.push(b' ');
        assert!(!is_probe(&long), "too long");
        assert!(!is_probe(&[b' '; PROBE_BYTES]), "blank");
        assert!(!is_probe(&[0xFF; PROBE_BYTES]), "noise");
        let mut other = br#"{"cnp":"here","v":1}"#.to_vec();
        other.resize(PROBE_BYTES, b' ');
        assert!(!is_probe(&other), "another message");
        let mut future = br#"{"cnp":"probe","v":2}"#.to_vec();
        future.resize(PROBE_BYTES, b' ');
        assert!(!is_probe(&future), "another version");
    }

    #[test]
    fn the_longest_reply_is_no_bigger_than_a_probe() {
        // The worst case: the longest name, made of the widest characters and the characters
        // JSON must escape (quotes and backslashes are removed from names, so none expand), the
        // highest port, a pairing window open.
        for name in [
            "a".repeat(100),
            "😀".repeat(100),
            "é".repeat(100),
            "\"\\\n\t".repeat(50),
        ] {
            let mut a = announcement(&clean_name(&name));
            a.pairing = true;
            let reply = a.encode().expect("fits");
            assert!(reply.len() <= PROBE_BYTES, "{} bytes", reply.len());
        }
    }

    #[test]
    fn a_reply_reads_back_and_a_forged_one_does_not() {
        let a = announcement("study");
        assert_eq!(Announcement::parse(&a.encode().unwrap()), Some(a.clone()));
        let good = String::from_utf8(a.encode().unwrap()).unwrap();
        for bad in [
            "".to_string(),
            "not json".to_string(),
            good.replace("here", "probe"),
            good.replace(r#""v":1"#, r#""v":2"#),
            good.replace("65535", "0"),
            good.replace("65535", "70000"),
            good.replace(&a.fingerprint.to_string(), "AB:CD"),
            good.replace(r#""pairing":false"#, r#""pairing":"no""#),
            good.replace(
                r#""name":"study""#,
                &format!(r#""name":"{}""#, "x".repeat(33)),
            ),
            format!("{good}{}", " ".repeat(PROBE_BYTES)),
        ] {
            assert_eq!(Announcement::parse(bad.as_bytes()), None, "{bad}");
        }
    }

    #[test]
    fn names_are_cleaned_and_cut_at_a_character() {
        assert_eq!(clean_name("  Carlos's   study  "), "Carlos's study");
        assert_eq!(clean_name("a\"b\\c\nd"), "abcd");
        assert_eq!(clean_name(""), "cb2026");
        assert_eq!(clean_name("\n\t"), "cb2026");
        let long = clean_name(&"😀".repeat(20));
        assert_eq!(long.len(), 32);
        assert_eq!(long.chars().count(), 8);
        assert_eq!(clean_name(&"x".repeat(100)).len(), NAME_BYTES);
    }

    #[test]
    fn a_sender_gets_a_few_replies_a_second_and_the_next_second_starts_fresh() {
        let t0 = Instant::now();
        let mut limit = RateLimit::new(t0);
        let one: IpAddr = "10.0.0.1".parse().unwrap();
        let two: IpAddr = "10.0.0.2".parse().unwrap();
        let answered = (0..50).filter(|_| limit.allow(one, t0)).count();
        assert_eq!(answered, PER_SENDER as usize);
        assert!(limit.allow(two, t0), "another sender isn't affected");
        assert!(limit.allow(one, t0 + Duration::from_millis(1_000)));
    }

    #[test]
    fn everyone_together_gets_a_bounded_number_of_replies() {
        let t0 = Instant::now();
        let mut limit = RateLimit::new(t0);
        let answered = (0..=255u8)
            .flat_map(|a| (0..4).map(move |b| IpAddr::from([10, 0, b, a])))
            .filter(|ip| limit.allow(*ip, t0))
            .count();
        assert_eq!(answered, OVERALL as usize);
    }

    #[test]
    fn the_sender_table_cannot_grow_without_end() {
        let t0 = Instant::now();
        let mut limit = RateLimit::new(t0);
        // Many addresses, a few probes each, within one second: the table stays bounded.
        for n in 0..(TRACKED as u32 * 3) {
            limit.overall = 0; // lift the overall cap to exercise the table's own
            limit.allow(IpAddr::from(n.to_be_bytes()), t0);
        }
        assert!(limit.senders.len() <= TRACKED);
    }
}
