//! The shell's commands that need the real binary: `match` against another engine program,
//! and switching the session to a protocol with `uci` or `xboard`.

use std::io::Write;
use std::process::{Command, Stdio};

/// Runs `cb2026 --shell` with `script` as its input and returns what it printed.
fn shell(script: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .arg("--shell")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start cb2026");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("cb2026 exits");
    assert!(output.status.success(), "exit status {}", output.status);
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn match_against_another_engine_program() {
    let pgn = std::env::temp_dir().join(format!("cb2026-cli-match-{}.pgn", std::process::id()));
    let pgn = pgn.to_str().unwrap();
    let out = shell(&format!(
        "level 5\nmatch {} 2 depth=2 pgn {pgn}\nquit\n",
        env!("CARGO_BIN_EXE_cb2026")
    ));
    let ours = format!("cb2026 {} (shell)", env!("CARGO_PKG_VERSION"));
    assert!(out.contains(&format!("{ours} vs cb2026")), "{out}");
    assert_eq!(out.matches("\nGame ").count(), 2, "{out}");
    assert!(out.contains(&format!("Match over: {ours} scored")), "{out}");
    let saved = std::fs::read_to_string(pgn).unwrap();
    assert_eq!(saved.matches("[White ").count(), 2);
    std::fs::remove_file(pgn).unwrap();
}

#[test]
fn switching_to_uci_and_xboard() {
    let out = shell("e4\nuci\nisready\nposition startpos\ngo depth 2\nquit\n");
    assert!(out.contains("\nuciok\n"), "{out}");
    assert!(out.contains("\nreadyok\n"), "{out}");
    assert!(out.contains("\nbestmove "), "{out}");
    let out = shell("xboard\nprotover 2\nping 4\nquit\n");
    assert!(out.contains("feature done=1"), "{out}");
    assert!(out.contains("pong 4"), "{out}");
}
