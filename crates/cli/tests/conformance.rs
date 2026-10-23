//! Protocol conformance: scripted UCI and XBoard sessions against the real `cb2026` binary.
//!
//! Claude can't operate GUIs, so these tests stand in for them (see `docs/PRD.md`,
//! "Compatibility matrix" and `docs/gui-compat.md`). Each one plays a session the way some GUI
//! does, including the awkward habits real GUIs have, and checks every response that matters.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How long a whole session may take before the test fails instead of hanging.
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);

/// One step of a script: a line to send, then how long to wait before the next.
struct Step<'a>(&'a str, u64);

/// A line the engine printed, and when (since the session started).
struct Line {
    text: String,
    at: Duration,
}

/// Runs `cb2026` with no arguments, sends each step's line (with `ending` after it), closes
/// standard input, and returns everything it printed.
fn run(steps: &[Step], ending: &str) -> Vec<Line> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start cb2026");
    let start = Instant::now();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(text) = line else { break };
            let _ = sender.send(Line {
                text,
                at: start.elapsed(),
            });
        }
    });
    let mut stdin = child.stdin.take().unwrap();
    for Step(line, pause) in steps {
        // The engine may already have quit; later writes then fail, which is fine.
        let _ = write!(stdin, "{line}{ending}");
        let _ = stdin.flush();
        thread::sleep(Duration::from_millis(*pause));
    }
    drop(stdin);
    let mut lines = Vec::new();
    while let Ok(line) = receiver.recv_timeout(SESSION_TIMEOUT.saturating_sub(start.elapsed())) {
        lines.push(line);
    }
    let status = child.wait().expect("cb2026 exits");
    reader.join().unwrap();
    assert!(status.success(), "cb2026 exited with {status}");
    lines
}

fn texts(lines: &[Line]) -> Vec<&str> {
    lines.iter().map(|l| l.text.as_str()).collect()
}

/// The `bestmove` lines' moves.
fn bestmoves(lines: &[Line]) -> Vec<&str> {
    lines
        .iter()
        .filter_map(|l| l.text.strip_prefix("bestmove "))
        .map(|rest| rest.split_whitespace().next().unwrap_or(""))
        .collect()
}

fn position_of(lines: &[Line], text: &str) -> usize {
    lines
        .iter()
        .position(|l| l.text == text)
        .unwrap_or_else(|| panic!("no {text:?} in {:?}", texts(lines)))
}

#[test]
fn uci_handshake_with_windows_line_endings() {
    let lines = run(
        &[
            Step("uci", 0),
            Step("isready", 0),
            Step("position startpos moves e2e4", 0),
            Step("go depth 3", 1500),
            Step("quit", 0),
        ],
        "\r\n",
    );
    assert!(position_of(&lines, "uciok") < position_of(&lines, "readyok"));
    assert!(lines.iter().any(|l| l.text.starts_with("id name cb2026")));
    let moves = bestmoves(&lines);
    assert_eq!(moves.len(), 1, "{:?}", texts(&lines));
    // A legal reply for Black.
    assert!(moves[0].starts_with(|c| ('a'..='h').contains(&c)) && moves[0].len() >= 4);
    assert!(
        moves[0][1..2] == *"7" || moves[0][1..2] == *"8",
        "{moves:?}"
    );
}

#[test]
fn uci_options_sent_mid_search() {
    // Some GUIs change options while the engine is thinking. The search must carry on (or
    // stop cleanly) and the engine must still answer `stop` and `isready`.
    let lines = run(
        &[
            Step("uci", 0),
            Step("position startpos", 0),
            Step("go infinite", 200),
            Step("setoption name Hash value 32", 0),
            Step("setoption name MultiPV value 2", 200),
            Step("stop", 300),
            Step("isready", 300),
            Step("quit", 0),
        ],
        "\n",
    );
    assert_eq!(bestmoves(&lines).len(), 1, "{:?}", texts(&lines));
    assert!(lines.iter().any(|l| l.text == "readyok"));
}

#[test]
fn uci_position_without_ucinewgame() {
    // Many GUIs never send `ucinewgame`; positions from different games just follow each other.
    let lines = run(
        &[
            Step("uci", 0),
            Step("position fen 6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1", 0),
            Step("go depth 3", 1500),
            Step("position startpos moves e2e4 e7e5", 0),
            Step("go depth 3", 1500),
            Step("quit", 0),
        ],
        "\n",
    );
    let moves = bestmoves(&lines);
    assert_eq!(moves.len(), 2, "{:?}", texts(&lines));
    assert_eq!(moves[0], "e1e8", "the back-rank mate");
    assert_ne!(moves[1], "e1e8");
}

#[test]
fn uci_very_short_clock() {
    // 50 ms left on the clock: the engine must still answer, fast.
    let lines = run(
        &[
            Step("uci", 0),
            Step("isready", 300),
            Step("position startpos", 0),
            Step("go wtime 50 btime 50", 1000),
            Step("quit", 0),
        ],
        "\n",
    );
    let moves = bestmoves(&lines);
    assert_eq!(moves.len(), 1, "{:?}", texts(&lines));
    assert_ne!(moves[0], "0000");
    let ready = &lines[position_of(&lines, "readyok")];
    let best = lines
        .iter()
        .find(|l| l.text.starts_with("bestmove"))
        .unwrap();
    // The `go` was sent 300 ms after `isready`; allow plenty for a slow CI machine.
    assert!(
        best.at < ready.at + Duration::from_millis(300 + 1000),
        "answered after {:?}",
        best.at - ready.at
    );
}

#[test]
fn uci_stop_straight_after_go() {
    let lines = run(
        &[
            Step("uci", 0),
            Step("position startpos", 0),
            Step("go wtime 600000 btime 600000", 0),
            Step("stop", 0),
            Step("isready", 500),
            Step("quit", 0),
        ],
        "\n",
    );
    let moves = bestmoves(&lines);
    assert_eq!(moves.len(), 1, "exactly one bestmove: {:?}", texts(&lines));
    assert_ne!(moves[0], "0000");
    assert!(position_of(&lines, "readyok") > 0);
}

#[test]
fn uci_game_over_position() {
    // Checkmated: there is no move to play, and the engine says so instead of hanging.
    let lines = run(
        &[
            Step("uci", 0),
            Step(
                "position fen rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
                0,
            ),
            Step("go depth 5", 500),
            Step("quit", 0),
        ],
        "\n",
    );
    assert_eq!(bestmoves(&lines), ["0000"], "{:?}", texts(&lines));
}

#[test]
fn uci_string_option_cleared_with_an_empty_value() {
    // A GUI clears a path option by sending `value` with nothing after it (some with a
    // trailing space); the option must be set to empty, not taken for an unknown one.
    let lines = run(
        &[
            Step("uci", 0),
            Step("setoption name BookFile value", 0),
            Step("setoption name SyzygyPath value ", 0),
            Step("isready", 300),
            Step("quit", 0),
        ],
        "\n",
    );
    let texts = texts(&lines);
    assert!(texts.contains(&"info string no opening book"), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t.contains("unknown option")),
        "{texts:?}"
    );
    assert!(texts.contains(&"readyok"));
}

#[test]
fn xboard_game_with_clock() {
    let lines = run(
        &[
            Step("xboard", 0),
            Step("protover 2", 0),
            Step("new", 0),
            Step("random", 0),
            Step("level 40 0:30 0", 0),
            Step("post", 0),
            Step("hard", 0),
            // 10 s on the clock: about a quarter of a second for this move, so even a debug
            // build answers well inside the wait below.
            Step("time 1000", 0),
            Step("otim 1000", 0),
            Step("usermove e2e4", 2500),
            Step("ping 1", 300),
            Step("quit", 0),
        ],
        "\n",
    );
    assert!(lines.iter().any(|l| l.text == "feature done=1"));
    let moves: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.text.strip_prefix("move "))
        .collect();
    assert_eq!(moves.len(), 1, "{:?}", texts(&lines));
    // Thinking output: ply score time nodes pv.
    let thinking = lines
        .iter()
        .find(|l| l.text.starts_with("1 "))
        .expect("thinking output");
    let fields: Vec<&str> = thinking.text.split_whitespace().collect();
    assert!(fields.len() >= 5, "{fields:?}");
    assert!(fields[1..4].iter().all(|f| f.parse::<i64>().is_ok()));
    assert!(lines.iter().any(|l| l.text == "pong 1"));
}

#[test]
fn xboard_force_setboard_and_windows_line_endings() {
    let lines = run(
        &[
            Step("xboard", 0),
            Step("protover 2", 0),
            Step("new", 0),
            Step("force", 0),
            Step("setboard 6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1", 0),
            Step("sd 3", 0),
            Step("go", 1500),
            Step("ping 2", 300),
            Step("quit", 0),
        ],
        "\r\n",
    );
    let moves: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.text.strip_prefix("move "))
        .collect();
    assert_eq!(moves, ["e1e8"], "{:?}", texts(&lines));
    assert!(lines.iter().any(|l| l.text == "pong 2"));
}
