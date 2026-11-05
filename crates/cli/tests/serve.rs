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

#[test]
fn serve_refuses_an_address_that_is_not_loopback() {
    let output = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .args(["--serve", "0.0.0.0:0"])
        .output()
        .expect("cb2026 runs");
    assert_eq!(output.status.code(), Some(1));
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("loopback"), "{said}");
}
