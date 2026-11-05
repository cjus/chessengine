//! `cb2026 --serve`, started as a program of its own and spoken to over loopback. Only in a build
//! with the `net` feature; the listener itself is tested in `crates/net`.

#![cfg(feature = "net")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

/// `cb2026 --serve ...` running, killed when dropped.
struct Served {
    child: Child,
    address: String,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts `cb2026 --serve 127.0.0.1:0` and reads the address it says it listens on.
fn serve(extra: &[&str]) -> Served {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--serve", "127.0.0.1:0"])
        .args(extra)
        .stderr(Stdio::piped())
        .spawn()
        .expect("cb2026 starts");
    let mut stderr = BufReader::new(child.stderr.take().expect("its stderr"));
    let mut line = String::new();
    stderr
        .read_line(&mut line)
        .expect("it says where it listens");
    let address = line
        .split_whitespace()
        .find(|word| word.starts_with("127.0.0.1:"))
        .unwrap_or_else(|| panic!("no address in {line:?}"))
        .trim_end_matches(',')
        .to_string();
    Served { child, address }
}

fn ask(address: &str, request: &str) -> String {
    let stream = TcpStream::connect(address).expect("connects");
    let mut writer = stream.try_clone().unwrap();
    writeln!(writer, "{request}").unwrap();
    let mut answer = String::new();
    BufReader::new(stream)
        .read_line(&mut answer)
        .expect("an answer");
    answer
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":"1.0","client":{"name":"test","version":"0"}}}"#;

#[test]
fn serve_answers_the_protocol_over_loopback() {
    let served = serve(&["--threads", "2", "--hash", "32"]);
    let answer = ask(&served.address, INITIALIZE);
    assert!(answer.contains(r#""protocol":"1.0""#), "{answer}");
    assert!(answer.contains(r#""name":"cb2026""#), "{answer}");
}

/// A directory for one test's node files.
fn data_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cb2026-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Lines of a child's stderr until one contains `needle`.
fn read_until(reader: &mut impl BufRead, needle: &str) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        assert!(
            reader.read_line(&mut line).expect("a line") > 0,
            "ended before {needle:?}: {lines:?}"
        );
        let done = line.contains(needle);
        lines.push(line);
        if done {
            return lines;
        }
    }
}

#[test]
fn an_address_beyond_loopback_is_served_encrypted_and_never_in_plaintext() {
    let dir = data_dir("beyond");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--serve", "0.0.0.0:0", "--data-dir"])
        .arg(&dir)
        .stderr(Stdio::piped())
        .spawn()
        .expect("cb2026 starts");
    let mut stderr = BufReader::new(child.stderr.take().expect("its stderr"));
    let said = read_until(&mut stderr, "cb2026 --pair").join("");
    assert!(
        said.contains("encrypted, for approved clients only"),
        "{said}"
    );
    assert!(said.contains("fingerprint"), "{said}");
    let port = said
        .split_whitespace()
        .find_map(|w| w.strip_prefix("0.0.0.0:"))
        .map(|p| p.trim_end_matches(',').to_string())
        .unwrap_or_else(|| panic!("no port in {said}"));
    // A plaintext client gets no protocol: the first thing it can hear is a TLS alert.
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connects");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    writeln!(stream, "{INITIALIZE}").unwrap();
    let mut heard = [0u8; 64];
    let n = std::io::Read::read(&mut stream, &mut heard).unwrap_or(0);
    assert!(n == 0 || heard[0] == 0x15, "{:?}", &heard[..n]);
    // Its files are in the directory it was given, readable by its owner alone.
    assert!(dir.join("identity.pem").exists());
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn pair_approves_a_client_that_then_uses_the_listener() {
    use cb2026_net::client;
    let dir = data_dir("pair");
    let mut server = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--serve", "127.0.0.1:0", "--tls", "--data-dir"])
        .arg(&dir)
        .stderr(Stdio::piped())
        .spawn()
        .expect("cb2026 starts");
    let mut stderr = BufReader::new(server.stderr.take().expect("its stderr"));
    let said = read_until(&mut stderr, "cb2026 --pair").join("");
    let address: std::net::SocketAddr = said
        .split_whitespace()
        .find(|w| w.starts_with("127.0.0.1:"))
        .expect("an address")
        .trim_end_matches(',')
        .parse()
        .expect("it parses");

    let mut pairing = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--pair", "--data-dir"])
        .arg(&dir)
        .stdout(Stdio::piped())
        .spawn()
        .expect("--pair starts");
    let mut shown = BufReader::new(pairing.stdout.take().expect("its output"));
    let lines = read_until(&mut shown, "Waiting for the client");
    let text = lines.join("");
    // The code, in two groups of four digits, is on the screen with the fingerprint.
    let groups: Vec<&str> = text
        .lines()
        .filter(|l| {
            let l = l.trim();
            l.len() == 9
                && l.as_bytes()[4] == b' '
                && l.bytes().all(|b| b.is_ascii_digit() || b == b' ')
        })
        .collect();
    assert_eq!(groups.len(), 1, "{text}");
    let code = groups[0].trim().to_string();
    let fingerprint: cb2026_net::Fingerprint = text
        .lines()
        .filter(|l| l.starts_with("     ") && l.contains(':'))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(":")
        .parse()
        .expect("the fingerprint on the screen reads back");

    let me = cb2026_net::Identity::generate().unwrap();
    let mut first = client::connect(address, &me, None).expect("connects");
    assert_eq!(
        first.server_fingerprint(),
        fingerprint,
        "the screen matches the node"
    );
    assert!(first.pair(&code, "a test client").unwrap());

    let mut rest = String::new();
    std::io::Read::read_to_string(&mut shown, &mut rest).unwrap();
    assert!(rest.contains("Paired: a test client"), "{rest}");
    assert!(pairing.wait().unwrap().success());

    let mut again = client::connect(address, &me, Some(fingerprint)).expect("connects");
    again.send_line(INITIALIZE).unwrap();
    let answer = again.read_line().unwrap().expect("an answer");
    assert!(answer.contains(r#""name":"cb2026""#), "{answer}");
    let _ = server.kill();
    let _ = server.wait();
}

#[test]
fn pair_reports_a_window_that_closed_without_a_client() {
    use cb2026_net::client;
    let dir = data_dir("pair-fails");
    let mut server = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--serve", "127.0.0.1:0", "--tls", "--data-dir"])
        .arg(&dir)
        .stderr(Stdio::piped())
        .spawn()
        .expect("cb2026 starts");
    let mut stderr = BufReader::new(server.stderr.take().expect("its stderr"));
    let said = read_until(&mut stderr, "cb2026 --pair").join("");
    let address: std::net::SocketAddr = said
        .split_whitespace()
        .find(|w| w.starts_with("127.0.0.1:"))
        .expect("an address")
        .trim_end_matches(',')
        .parse()
        .expect("it parses");
    let mut pairing = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--pair", "--data-dir"])
        .arg(&dir)
        .stdout(Stdio::piped())
        .spawn()
        .expect("--pair starts");
    let mut shown = BufReader::new(pairing.stdout.take().expect("its output"));
    read_until(&mut shown, "Waiting for the client");
    // Wrong codes, as often as a window allows.
    let me = cb2026_net::Identity::generate().unwrap();
    for _ in 0..cb2026_net::trust::PAIRING_ATTEMPTS {
        let mut c = client::connect(address, &me, None).expect("connects");
        // Eight letters can never be a code, so this is wrong whatever the code is.
        assert!(!c.pair("abcdefgh", "me").unwrap());
    }
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut shown, &mut rest).unwrap();
    assert!(rest.contains("nobody paired"), "{rest}");
    assert_eq!(pairing.wait().unwrap().code(), Some(1));
    let _ = server.kill();
    let _ = server.wait();
}

#[test]
fn discover_needs_the_encrypted_listener() {
    let output = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--serve", "127.0.0.1:0", "--discover"])
        .output()
        .expect("cb2026 runs");
    assert_eq!(output.status.code(), Some(2));
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("--discover needs"), "{said}");
}

#[test]
fn a_discovering_node_answers_a_clients_search_under_its_name() {
    use cb2026_net::client;
    let dir = data_dir("discover");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args([
            "--serve",
            "127.0.0.1:0",
            "--tls",
            "--discover",
            "--name",
            "den",
            "--data-dir",
        ])
        .arg(&dir)
        .stderr(Stdio::piped())
        .spawn()
        .expect("cb2026 starts");
    let mut stderr = BufReader::new(child.stderr.take().expect("its stderr"));
    let mut said = String::new();
    // The line that ends the announcement says to run --pair; discovery's comes before it.
    let mut line = String::new();
    while stderr.read_line(&mut line).unwrap_or(0) > 0 {
        said.push_str(&line);
        if line.contains("cb2026 --pair")
            || line.starts_with("cb2026: ") && line.contains("Address already in use")
        {
            break;
        }
        line.clear();
    }
    if !said.contains("Answering discovery probes") {
        // The fixed UDP port is taken (another cb2026 is answering on this computer): nothing
        // to test here, and the node said so.
        eprintln!("skipped: {said}");
        let _ = child.kill();
        let _ = child.wait();
        return;
    }
    assert!(said.contains("as \"den\""), "{said}");
    let target = std::net::SocketAddr::from(([127, 0, 0, 1], cb2026_net::discovery::PORT));
    let found = client::discover_at(&[target], std::time::Duration::from_millis(800)).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].announcement.name, "den");
    let _ = child.kill();
    let _ = child.wait();
}

/// An encrypted node on loopback with its pairing window open, and the code the window shows.
/// Everything is killed when dropped.
struct Pairable {
    server: Child,
    pairing: Child,
    address: String,
    code: String,
}

impl Drop for Pairable {
    fn drop(&mut self) {
        for child in [&mut self.server, &mut self.pairing] {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn pairable(dir: &std::path::Path) -> Pairable {
    let mut server = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--serve", "127.0.0.1:0", "--tls", "--data-dir"])
        .arg(dir)
        .stderr(Stdio::piped())
        .spawn()
        .expect("cb2026 starts");
    let mut stderr = BufReader::new(server.stderr.take().expect("its stderr"));
    let said = read_until(&mut stderr, "cb2026 --pair").join("");
    let address = said
        .split_whitespace()
        .find(|w| w.starts_with("127.0.0.1:"))
        .expect("an address")
        .trim_end_matches(',')
        .to_string();
    let mut pairing = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--pair", "--data-dir"])
        .arg(dir)
        .stdout(Stdio::piped())
        .spawn()
        .expect("--pair starts");
    let mut shown = BufReader::new(pairing.stdout.take().expect("its output"));
    let text = read_until(&mut shown, "Waiting for the client").join("");
    let code = text
        .lines()
        .map(str::trim)
        .find(|l| {
            l.len() == 9
                && l.as_bytes()[4] == b' '
                && l.bytes().all(|b| b.is_ascii_digit() || b == b' ')
        })
        .expect("the code")
        .to_string();
    // Keep reading its output so it never blocks on a full pipe.
    std::thread::spawn(move || {
        let mut sink = String::new();
        let _ = std::io::Read::read_to_string(&mut shown, &mut sink);
    });
    Pairable {
        server,
        pairing,
        address,
        code,
    }
}

/// `cb2026 --connect ADDRESS --data-dir DIR`, given `typed` as what the person types.
fn connect(address: &str, dir: &std::path::Path, typed: &str) -> std::process::Output {
    connect_slowly(address, dir, typed, std::time::Duration::ZERO)
}

/// [`connect`], with `pause` between the answer to the fingerprint question (the first line)
/// and the code (the rest): a person comparing fingerprints takes a while.
fn connect_slowly(
    address: &str,
    dir: &std::path::Path,
    typed: &str,
    pause: std::time::Duration,
) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--connect", address, "--data-dir"])
        .arg(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("--connect starts");
    let mut stdin = child.stdin.take().expect("its input");
    let (first, rest) = typed.split_once('\n').unwrap_or((typed, ""));
    stdin.write_all(format!("{first}\n").as_bytes()).unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(pause);
    stdin.write_all(rest.as_bytes()).unwrap();
    drop(stdin);
    child.wait_with_output().expect("--connect ends")
}

#[test]
fn connect_pairs_once_and_then_connects_without_a_code() {
    let node_dir = data_dir("connect-node");
    let mine = data_dir("connect-mine");
    let node = pairable(&node_dir);

    let first = connect(&node.address, &mine, &format!("yes\n{}\n", node.code));
    let said = String::from_utf8_lossy(&first.stdout);
    assert!(
        first.status.success(),
        "{said}\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(said.contains("Paired."), "{said}");
    assert!(said.contains("Best move "), "{said}");
    assert!(said.contains("1 thread(s)"), "{said}");
    // The node's screen and this client agree on who the node is.
    let identity = cb2026_net::Store::new(&node_dir).identity().unwrap();
    assert!(
        said.contains(
            &identity
                .fingerprint()
                .in_lines()
                .lines()
                .next()
                .unwrap()
                .to_string()
        ),
        "{said}"
    );
    assert!(mine.join("client-identity.pem").exists());

    // The second time nothing is typed.
    let second = connect(&node.address, &mine, "");
    let said = String::from_utf8_lossy(&second.stdout);
    assert!(
        second.status.success(),
        "{said}\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(said.contains("paired with before"), "{said}");
    assert!(said.contains("Best move "), "{said}");
}

#[test]
fn connect_sends_no_code_unless_the_person_says_the_fingerprints_match() {
    let node_dir = data_dir("connect-decline-node");
    let mine = data_dir("connect-decline-mine");
    let node = pairable(&node_dir);
    // The code is typed here, but the answer to the fingerprint question is no.
    let output = connect(&node.address, &mine, &format!("no\n{}\n", node.code));
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{said}");
    assert!(said.contains("Nothing was sent"), "{said}");
    assert!(!said.contains("Best move"), "{said}");
    // The node never saw a pairing line, so its window and approvals are untouched.
    let store = cb2026_net::Store::new(&node_dir);
    assert!(store.approved().unwrap().is_empty());
    assert!(store.pairing_is_open(std::time::SystemTime::now()));
}

#[test]
fn connect_reports_a_wrong_code_and_remembers_nothing() {
    let node_dir = data_dir("connect-wrong-node");
    let mine = data_dir("connect-wrong-mine");
    let node = pairable(&node_dir);
    let output = connect(&node.address, &mine, "yes\n00000000\n");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{said}");
    assert!(said.contains("didn't accept the code"), "{said}");
    assert!(!mine.join("engines.txt").exists());
    assert!(
        cb2026_net::Store::new(&node_dir)
            .approved()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn connect_says_so_when_nothing_answers_at_an_address() {
    let mine = data_dir("connect-nothing");
    // Port 1 on loopback has nothing listening.
    let output = connect("127.0.0.1:1", &mine, "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("couldn't connect"));
}

#[test]
fn connect_pairs_even_when_the_person_takes_longer_than_the_nodes_deadline() {
    let node_dir = data_dir("connect-slow-node");
    let mine = data_dir("connect-slow-mine");
    let node = pairable(&node_dir);
    // The node gives a connection ten seconds (cb2026_net::HANDSHAKE_TIMEOUT) to send its first
    // message. This person takes longer, so the client must not hold a connection open meanwhile.
    let pause = cb2026_net::HANDSHAKE_TIMEOUT + std::time::Duration::from_secs(2);
    let output = connect_slowly(
        &node.address,
        &mine,
        &format!("yes\n{}\n", node.code),
        pause,
    );
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{said}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("Paired."), "{said}");
    assert!(said.contains("Best move "), "{said}");
}
