//! A fake UCI engine for testing the match runner: it plays its first legal move at once, and
//! can be told to crash at a chosen moment.
//!
//! It is a test helper, not an engine. `crates/tools/tests/match_failures.rs` runs it as a
//! process, the way the `match` tool runs real engines, to check what the runner does when an
//! engine dies (cjus/chessengine#89). Its UCI options:
//!
//! - `FailStart`, `FailNewGame`, `FailGo`: when to exit, as if it had crashed: on the
//!   handshake's `isready`, on `ucinewgame`, or on `go`. `N` exits at the Nth such moment,
//!   `N+` at the Nth and every one after it.
//! - `Counter`: a path prefix. The counts are kept in the files `<Counter>.start`,
//!   `<Counter>.newgame` and `<Counter>.go`, so they carry on across the processes a match
//!   starts one after another (with one worker). Without it, each process counts from zero.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use cb2026_chess::{Position, STARTING_FEN};

fn main() -> io::Result<()> {
    let mut fake = Fake {
        options: HashMap::new(),
        counts: HashMap::new(),
        started: false,
        position: Position::from_fen(STARTING_FEN).expect("valid"),
    };
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        let (command, rest) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
        match command {
            "uci" => {
                writeln!(out, "id name fake-uci")?;
                writeln!(out, "uciok")?;
            }
            "setoption" => fake.set_option(rest),
            "isready" => {
                if !fake.started {
                    fake.started = true;
                    fake.moment("start", "FailStart");
                }
                writeln!(out, "readyok")?;
            }
            "ucinewgame" => fake.moment("newgame", "FailNewGame"),
            "position" => fake.set_position(rest),
            "go" => {
                fake.moment("go", "FailGo");
                let mv = fake.position.legal_moves().first().copied();
                let uci = mv.map_or("0000".to_string(), |mv| mv.to_uci(false));
                writeln!(out, "bestmove {uci}")?;
            }
            "quit" => break,
            _ => {}
        }
        out.flush()?;
    }
    Ok(())
}

struct Fake {
    options: HashMap<String, String>,
    /// Each moment's count in this process, when there is no `Counter`.
    counts: HashMap<&'static str, u64>,
    /// Whether the handshake's `isready` has come.
    started: bool,
    position: Position,
}

impl Fake {
    /// `name <name> value <value>`.
    fn set_option(&mut self, text: &str) {
        let text = text.strip_prefix("name ").unwrap_or(text);
        let (name, value) = text.split_once(" value ").unwrap_or((text, ""));
        self.options
            .insert(name.trim().to_string(), value.trim().to_string());
    }

    /// `fen <FEN> [moves ...]` or `startpos [moves ...]`.
    fn set_position(&mut self, text: &str) {
        let (start, moves) = text.split_once(" moves ").unwrap_or((text, ""));
        let fen = start.strip_prefix("fen ").unwrap_or(STARTING_FEN);
        let Ok(mut position) = Position::from_fen(fen.trim()) else {
            return;
        };
        for uci in moves.split_whitespace() {
            match position.parse_uci_move(uci) {
                Some(mv) => position.make_move(mv),
                None => return,
            }
        }
        self.position = position;
    }

    /// Counts one `event` and exits if the option `fail` says to fail now.
    fn moment(&mut self, event: &'static str, fail: &str) {
        let count = match self.options.get("Counter") {
            Some(prefix) => {
                let path = format!("{prefix}.{event}");
                let count = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| text.trim().parse::<u64>().ok())
                    .unwrap_or(0)
                    + 1;
                std::fs::write(&path, count.to_string()).expect("the counter file is writable");
                count
            }
            None => {
                let count = self.counts.entry(event).or_insert(0);
                *count += 1;
                *count
            }
        };
        let Some(spec) = self.options.get(fail) else {
            return;
        };
        let (at, and_after) = match spec.strip_suffix('+') {
            Some(at) => (at, true),
            None => (spec.as_str(), false),
        };
        if let Ok(at) = at.parse::<u64>()
            && (count == at || (and_after && count > at))
        {
            std::process::exit(1);
        }
    }
}
