//! The Universal Chess Interface.
//!
//! A UCI session looks like this (`>` is the GUI, `<` is the engine):
//!
//! ```text
//! > uci
//! < id name cb2026 0.1.0
//! < id author the cb2026 authors
//! < option name Hash type spin default 16 min 1 max 65536
//! < option name Clear Hash type button
//! < option name Threads type spin default 1 min 1 max 256
//! < option name SyzygyPath type string default <empty>
//! < option name SyzygyProbeDepth type spin default 1 min 1 max 100
//! < option name SyzygyProbeLimit type spin default 7 min 0 max 7
//! < option name OwnBook type check default false
//! < option name BookFile type string default <empty>
//! < option name MultiPV type spin default 1 min 1 max 256
//! < option name Skill Level type spin default 20 min 0 max 20
//! < option name UCI_LimitStrength type check default false
//! < option name UCI_Elo type spin default 2400 min 1000 max 2400
//! < option name Move Overhead type spin default 30 min 0 max 5000
//! < option name UCI_Chess960 type check default false
//! < uciok
//! > isready
//! < readyok
//! > position startpos moves e2e4 e7e5
//! > go wtime 60000 btime 60000 winc 1000 binc 1000
//! < info depth 1 seldepth 1 score cp 0 nodes 29 nps 29000 time 1 hashfull 0 pv g1f3
//! < bestmove g1f3
//! > quit
//! ```
//!
//! The GUI owns the game: before each search it sends the whole position (`position ...
//! moves ...`), then `go` with the clock. The engine searches on a **separate thread**, so it
//! can answer `isready` and act on `stop` immediately while thinking. When the search ends
//! the engine prints `bestmove`; after `go infinite` it waits for `stop` first, as the
//! protocol requires.
//!
//! See the Chess Programming Wiki, [UCI](https://www.chessprogramming.org/UCI).

use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cb2026_chess::{Move, Position};
use cb2026_engine::book::OpeningBook;
use cb2026_engine::levels::{ELO_RANGE, FULL_STRENGTH};
use cb2026_engine::tablebase::TablebaseLoader;
use cb2026_engine::tt::{DEFAULT_HASH_MB, MAX_HASH_MB};
use cb2026_engine::{AUTHOR, Engine, Limits, MAX_THREADS, SearchInfo, id_name};

/// Where the engine's replies go. Shared between the command loop and the search thread.
type Output = Arc<Mutex<Box<dyn Write + Send>>>;

/// Writes one line to the GUI and flushes it straight away (GUIs wait for whole lines).
fn send(output: &Output, line: &str) {
    let mut out = output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // If the GUI has gone away there is no one left to tell, so errors are ignored here; the
    // command loop notices the closed input and exits.
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// UCI options this engine understands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Options {
    chess960: bool,
}

/// A search running on its own thread.
struct RunningSearch {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<Engine>,
}

/// The state of one UCI session.
struct Session {
    output: Output,
    /// The engine, when it isn't busy searching (a running search owns it until it ends).
    engine: Option<Engine>,
    /// How the engine opens tablebases (kept to rebuild the engine if a search thread fails).
    tablebase_loader: Option<TablebaseLoader>,
    search: Option<RunningSearch>,
    position: Position,
    /// Hash keys of the positions before `position` in the game, for repetition detection.
    history: Vec<u64>,
    options: Options,
}

/// Runs a UCI session: reads commands from `input` until `quit` or the end of the input.
///
/// `first_line` is a line already read from the input (the `uci` that told the binary which
/// protocol to speak); it is handled first.
pub fn run<R: BufRead>(
    input: R,
    output: Box<dyn Write + Send>,
    first_line: Option<String>,
) -> io::Result<()> {
    run_with_engine(input, output, first_line, Engine::new())
}

/// Like [`run`], with a given engine (tests use a seeded one so its moves are predictable).
pub fn run_with_engine<R: BufRead>(
    input: R,
    output: Box<dyn Write + Send>,
    first_line: Option<String>,
    engine: Engine,
) -> io::Result<()> {
    let mut session = Session {
        output: Arc::new(Mutex::new(output)),
        tablebase_loader: engine.tablebase_loader,
        engine: Some(engine),
        search: None,
        position: Position::startpos(),
        history: Vec::new(),
        options: Options::default(),
    };
    for line in first_line.into_iter().map(Ok).chain(input.lines()) {
        if !session.handle(line?.trim()) {
            break;
        }
    }
    // End of input or `quit`: stop any search so the process can exit.
    session.stop_search();
    Ok(())
}

impl Session {
    /// Handles one command line. Returns false when the session should end.
    fn handle(&mut self, line: &str) -> bool {
        let mut words = line.split_whitespace();
        let Some(command) = words.next() else {
            return true;
        };
        let args: Vec<&str> = words.collect();
        match command {
            "uci" => {
                send(&self.output, &format!("id name {}", id_name()));
                send(&self.output, &format!("id author {AUTHOR}"));
                send(
                    &self.output,
                    &format!(
                        "option name Hash type spin default {DEFAULT_HASH_MB} min 1 max {MAX_HASH_MB}"
                    ),
                );
                send(&self.output, "option name Clear Hash type button");
                send(
                    &self.output,
                    &format!("option name Threads type spin default 1 min 1 max {MAX_THREADS}"),
                );
                send(
                    &self.output,
                    "option name SyzygyPath type string default <empty>",
                );
                send(
                    &self.output,
                    "option name SyzygyProbeDepth type spin default 1 min 1 max 100",
                );
                send(
                    &self.output,
                    "option name SyzygyProbeLimit type spin default 7 min 0 max 7",
                );
                send(&self.output, "option name OwnBook type check default false");
                send(
                    &self.output,
                    "option name BookFile type string default <empty>",
                );
                send(
                    &self.output,
                    "option name MultiPV type spin default 1 min 1 max 256",
                );
                send(
                    &self.output,
                    &format!(
                        "option name Skill Level type spin default {FULL_STRENGTH} min 0 max {FULL_STRENGTH}"
                    ),
                );
                send(
                    &self.output,
                    "option name UCI_LimitStrength type check default false",
                );
                send(
                    &self.output,
                    &format!(
                        "option name UCI_Elo type spin default {} min {} max {}",
                        ELO_RANGE.1, ELO_RANGE.0, ELO_RANGE.1
                    ),
                );
                send(
                    &self.output,
                    "option name Move Overhead type spin default 30 min 0 max 5000",
                );
                send(
                    &self.output,
                    "option name UCI_Chess960 type check default false",
                );
                send(&self.output, "uciok");
            }
            // Answered at once, even mid-search: the GUI uses it to check we're alive.
            "isready" => send(&self.output, "readyok"),
            "debug" => {}
            "setoption" => {
                self.stop_search();
                self.set_option(&args);
            }
            "ucinewgame" => {
                self.stop_search();
                if let Some(engine) = self.engine.as_mut() {
                    engine.new_game();
                }
            }
            "position" => {
                self.stop_search();
                if let Err(message) = self.set_position(&args) {
                    send(&self.output, &format!("info string {message}"));
                }
            }
            "go" => {
                self.stop_search();
                self.go(&args);
            }
            "stop" => self.stop_search(),
            // Pondering arrives with stage 17; until then a ponder search is a normal one.
            "ponderhit" => {}
            "quit" => return false,
            other => send(
                &self.output,
                &format!("info string unknown command '{other}'"),
            ),
        }
        true
    }

    /// `setoption name <name> [value <value>]`.
    fn set_option(&mut self, args: &[&str]) {
        let text = args.join(" ");
        let rest = text.strip_prefix("name ").unwrap_or(&text);
        let (name, value) = match rest.split_once(" value ") {
            Some((name, value)) => (name.trim(), value.trim()),
            None => (rest.trim(), ""),
        };
        // Option names are case-insensitive in UCI.
        match name.to_ascii_lowercase().as_str() {
            "uci_chess960" => self.options.chess960 = value.eq_ignore_ascii_case("true"),
            "hash" => match value.parse::<usize>() {
                Ok(mb) if (1..=MAX_HASH_MB).contains(&mb) => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.set_hash_size(mb);
                    }
                }
                _ => send(
                    &self.output,
                    &format!("info string Hash must be 1-{MAX_HASH_MB} (MB), not '{value}'"),
                ),
            },
            "threads" => match value.parse::<usize>() {
                Ok(n) if (1..=MAX_THREADS).contains(&n) => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.threads = n;
                    }
                }
                _ => send(
                    &self.output,
                    &format!("info string Threads must be 1-{MAX_THREADS}, not '{value}'"),
                ),
            },
            "syzygypath" => {
                if let Some(engine) = self.engine.as_mut() {
                    let message = match engine.load_tablebases(value) {
                        Ok(summary) => summary,
                        Err(e) => format!("tablebases not loaded: {e}"),
                    };
                    send(&self.output, &format!("info string {message}"));
                }
            }
            "syzygyprobedepth" => match value.parse::<u32>() {
                Ok(depth) if (1..=100).contains(&depth) => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.tablebase_params.probe_depth = depth;
                    }
                }
                _ => send(
                    &self.output,
                    &format!("info string SyzygyProbeDepth must be 1-100, not '{value}'"),
                ),
            },
            "multipv" => match value.parse::<usize>() {
                Ok(n) if (1..=256).contains(&n) => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.multipv = n;
                    }
                }
                _ => send(
                    &self.output,
                    &format!("info string MultiPV must be 1-256, not '{value}'"),
                ),
            },
            "skill level" => match value.parse::<u32>() {
                Ok(level) if level <= FULL_STRENGTH => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.skill_level = level;
                    }
                }
                _ => send(
                    &self.output,
                    &format!("info string Skill Level must be 0-{FULL_STRENGTH}, not '{value}'"),
                ),
            },
            "uci_limitstrength" => {
                if let Some(engine) = self.engine.as_mut() {
                    engine.limit_strength = value.eq_ignore_ascii_case("true");
                }
            }
            "uci_elo" => match value.parse::<u32>() {
                Ok(elo) if (ELO_RANGE.0..=ELO_RANGE.1).contains(&elo) => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.elo = elo;
                    }
                }
                _ => send(
                    &self.output,
                    &format!(
                        "info string UCI_Elo must be {}-{}, not '{value}'",
                        ELO_RANGE.0, ELO_RANGE.1
                    ),
                ),
            },
            "ownbook" => {
                if let Some(engine) = self.engine.as_mut() {
                    engine.own_book = value.eq_ignore_ascii_case("true");
                }
            }
            "bookfile" => {
                if let Some(engine) = self.engine.as_mut() {
                    let message = match load_book(value) {
                        Ok(Some(book)) => {
                            let message = format!("opening book with {} entries", book.len());
                            engine.book = Some(Arc::new(book));
                            message
                        }
                        Ok(None) => {
                            engine.book = None;
                            "no opening book".to_string()
                        }
                        Err(e) => format!("opening book not loaded: {e}"),
                    };
                    send(&self.output, &format!("info string {message}"));
                }
            }
            "syzygyprobelimit" => match value.parse::<u32>() {
                Ok(limit) if limit <= 7 => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.tablebase_params.probe_limit = limit;
                    }
                }
                _ => send(
                    &self.output,
                    &format!("info string SyzygyProbeLimit must be 0-7, not '{value}'"),
                ),
            },
            "clear hash" => {
                if let Some(engine) = self.engine.as_mut() {
                    engine.tt.clear();
                }
            }
            "move overhead" => match value.parse::<u64>() {
                Ok(ms) if ms <= 5000 => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.time.move_overhead = Duration::from_millis(ms);
                    }
                }
                _ => send(
                    &self.output,
                    &format!("info string Move Overhead must be 0-5000, not '{value}'"),
                ),
            },
            _ => send(
                &self.output,
                &format!("info string unknown option '{name}'"),
            ),
        }
    }

    /// `position startpos|fen <FEN> [moves <move> ...]`.
    fn set_position(&mut self, args: &[&str]) -> Result<(), String> {
        let moves_at = args.iter().position(|&w| w == "moves");
        let (setup, moves) = match moves_at {
            Some(i) => (&args[..i], &args[i + 1..]),
            None => (args, &[][..]),
        };
        let mut position = match setup {
            ["startpos"] => Position::startpos(),
            ["fen", fen @ ..] => {
                Position::from_fen(&fen.join(" ")).map_err(|e| format!("invalid FEN: {e}"))?
            }
            _ => return Err("expected 'position startpos' or 'position fen <FEN>'".to_string()),
        };
        let mut history = Vec::with_capacity(moves.len());
        for text in moves {
            let mv = position
                .parse_uci_move(text)
                .ok_or_else(|| format!("illegal move '{text}' in position command"))?;
            history.push(position.hash());
            position.make_move(mv);
        }
        self.position = position;
        self.history = history;
        Ok(())
    }

    /// `go [wtime N] [btime N] [winc N] [binc N] [movestogo N] [depth N] [nodes N] [mate N]
    /// [movetime N] [infinite] [ponder] [searchmoves <move> ...]`.
    fn go(&mut self, args: &[&str]) {
        let limits = parse_go(&self.position, args);
        let Some(mut engine) = self.engine.take() else {
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let output = Arc::clone(&self.output);
        let position = self.position.clone();
        let history = self.history.clone();
        let chess960 = self.options.chess960;
        // A ponder or infinite search must not report its move until told to stop.
        let wait_for_stop = limits.infinite || args.contains(&"ponder");

        let handle = thread::spawn(move || {
            let mut on_info = |info: &SearchInfo| send(&output, &info_line(info, chess960));
            let result = engine.search(&position, &history, &limits, &thread_stop, &mut on_info);
            while wait_for_stop && !thread_stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(1));
            }
            let mut line = match result.best_move {
                Some(mv) => format!("bestmove {}", mv.to_uci(chess960)),
                // No legal move: UCI has no standard answer; this is the common convention.
                None => "bestmove 0000".to_string(),
            };
            if let Some(ponder) = result.ponder_move {
                line.push_str(&format!(" ponder {}", ponder.to_uci(chess960)));
            }
            send(&output, &line);
            engine
        });
        self.search = Some(RunningSearch { stop, handle });
    }

    /// Stops the running search, if any, and waits for it to report its move.
    fn stop_search(&mut self) {
        if let Some(search) = self.search.take() {
            search.stop.store(true, Ordering::Relaxed);
            match search.handle.join() {
                Ok(engine) => self.engine = Some(engine),
                Err(_) => {
                    send(&self.output, "info string the search thread failed");
                    let mut engine = Engine::new();
                    engine.tablebase_loader = self.tablebase_loader;
                    self.engine = Some(engine);
                }
            }
        }
    }
}

/// Parses the arguments of `go` into search limits.
fn parse_go(position: &Position, args: &[&str]) -> Limits {
    let mut limits = Limits::default();
    let millis = |s: &&str| {
        s.parse::<i64>()
            .ok()
            .map(|ms| Duration::from_millis(ms.max(0) as u64))
    };
    let mut i = 0;
    while i < args.len() {
        let next = args.get(i + 1);
        match args[i] {
            "wtime" => limits.time[0] = next.and_then(millis),
            "btime" => limits.time[1] = next.and_then(millis),
            "winc" => limits.increment[0] = next.and_then(millis),
            "binc" => limits.increment[1] = next.and_then(millis),
            "movestogo" => limits.moves_to_go = next.and_then(|s| s.parse().ok()),
            "depth" => limits.depth = next.and_then(|s| s.parse().ok()),
            "nodes" => limits.nodes = next.and_then(|s| s.parse().ok()),
            "mate" => limits.mate = next.and_then(|s| s.parse().ok()),
            "movetime" => limits.movetime = next.and_then(millis),
            "infinite" => limits.infinite = true,
            "searchmoves" => {
                // Every following word that is a legal move.
                while let Some(mv) = args.get(i + 1).and_then(|s| position.parse_uci_move(s)) {
                    limits.search_moves.push(mv);
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    limits
}

/// Reads a Polyglot book from `path` (`None` for an empty path or `<empty>`).
fn load_book(path: &str) -> Result<Option<OpeningBook>, String> {
    if path.trim().is_empty() || path == "<empty>" {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    OpeningBook::from_bytes(&bytes).map(Some)
}

/// Formats progress as a UCI `info` line.
fn info_line(info: &SearchInfo, chess960: bool) -> String {
    let pv: Vec<String> = info
        .pv
        .iter()
        .map(|mv: &Move| mv.to_uci(chess960))
        .collect();
    let ms = info.time.as_millis();
    let nps = info.nodes as u128 * 1000 / ms.max(1);
    format!(
        "info depth {} seldepth {} multipv {} score {} nodes {} nps {nps} time {ms} hashfull {} tbhits {} pv {}",
        info.depth,
        info.seldepth,
        info.multipv,
        info.score,
        info.nodes,
        info.hashfull,
        info.tbhits,
        pv.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An output buffer the tests can read back after the session ends.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Runs a scripted session and returns the engine's output lines.
    fn session(script: &str) -> Vec<String> {
        let captured = Captured::default();
        run_with_engine(
            script.as_bytes(),
            Box::new(captured.clone()),
            None,
            Engine::new(),
        )
        .unwrap();
        let bytes = captured.0.lock().unwrap().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn bestmove(lines: &[String]) -> String {
        lines
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix("bestmove "))
            .and_then(|rest| rest.split_whitespace().next())
            .expect("a bestmove line")
            .to_string()
    }

    #[test]
    fn handshake() {
        let lines = session("uci\nisready\nquit\n");
        assert!(lines[0].starts_with("id name cb2026 "));
        assert!(lines.contains(&"uciok".to_string()));
        assert_eq!(lines.last().unwrap(), "readyok");
    }

    #[test]
    fn plays_a_legal_move_from_a_position() {
        let lines = session("position startpos moves e2e4 e7e5\ngo depth 1\nisready\n");
        let mut pos = Position::startpos();
        for uci in ["e2e4", "e7e5"] {
            pos.make_move(pos.parse_uci_move(uci).unwrap());
        }
        let reply = bestmove(&lines);
        assert!(pos.parse_uci_move(&reply).is_some(), "{reply} is legal");
        assert!(lines.iter().any(|l| l.starts_with("info depth 1 ")));
    }

    #[test]
    fn fen_positions_and_searchmoves() {
        let lines =
            session("position fen 4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\ngo searchmoves e2e3\nisready\n");
        assert_eq!(bestmove(&lines), "e2e3");
    }

    #[test]
    fn infinite_search_waits_for_stop() {
        let lines = session("position startpos\ngo infinite\nisready\nstop\n");
        let ready = lines.iter().position(|l| l == "readyok").unwrap();
        let best = lines
            .iter()
            .position(|l| l.starts_with("bestmove"))
            .unwrap();
        assert!(
            ready < best,
            "isready is answered while searching: {lines:?}"
        );
    }

    #[test]
    fn chess960_castling_notation() {
        // The same castling move, written e1g1 normally and e1h1 ("king takes rook") in
        // Chess960 mode.
        let fen = "4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1";
        let standard = session(&format!(
            "position fen {fen}\ngo searchmoves e1g1\nisready\n"
        ));
        assert_eq!(bestmove(&standard), "e1g1");
        let chess960 = session(&format!(
            "setoption name UCI_Chess960 value true\nposition fen {fen}\ngo searchmoves e1h1\nisready\n"
        ));
        assert_eq!(bestmove(&chess960), "e1h1");
    }

    #[test]
    fn mated_side_reports_no_move() {
        let lines = session("position startpos moves f2f3 e7e5 g2g4 d8h4\ngo\nisready\n");
        assert_eq!(bestmove(&lines), "0000");
    }

    #[test]
    fn bad_input_is_reported_not_fatal() {
        let lines = session(
            "position fen not a fen\nposition startpos moves e2e5\nfrobnicate\nsetoption name Frobnicity value 64\nisready\n",
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("info string invalid FEN"))
        );
        assert!(lines.iter().any(|l| l.contains("illegal move 'e2e5'")));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("unknown command 'frobnicate'"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("unknown option 'Frobnicity'"))
        );
        assert_eq!(lines.last().unwrap(), "readyok");
    }

    #[test]
    fn reports_every_depth_and_accepts_move_overhead() {
        let lines = session(
            "setoption name Move Overhead value 50\nposition startpos\ngo depth 3\nisready\n",
        );
        let depths = lines.iter().filter(|l| l.starts_with("info depth")).count();
        assert_eq!(depths, 3, "{lines:?}");
        assert!(!lines.iter().any(|l| l.contains("Move Overhead must")));
        let bad = session("setoption name Move Overhead value lots\nisready\n");
        assert!(
            bad.iter()
                .any(|l| l.contains("Move Overhead must be 0-5000"))
        );
    }

    #[test]
    fn hash_options() {
        let lines = session(
            "uci\nsetoption name Hash value 1\nsetoption name Clear Hash\nposition startpos\ngo depth 3\nisready\n",
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("option name Hash type spin default 16"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l == "option name Clear Hash type button")
        );
        assert!(lines.iter().any(|l| l.contains(" hashfull ")));
        assert!(!lines.iter().any(|l| l.starts_with("info string")));
        let bad = session("setoption name Hash value 0\nisready\n");
        assert!(bad.iter().any(|l| l.contains("Hash must be 1-65536")));
    }

    #[test]
    fn syzygy_options() {
        let lines = session(
            "uci\nsetoption name SyzygyPath value /no/tables\nsetoption name SyzygyProbeLimit value 5\nsetoption name SyzygyProbeDepth value 0\nisready\n",
        );
        assert!(
            lines
                .iter()
                .any(|l| l == "option name SyzygyPath type string default <empty>")
        );
        // The test engine has no tablebase loader, like a build without the syzygy feature.
        assert!(
            lines
                .iter()
                .any(|l| l.contains("tablebases not loaded: this build has no tablebase support"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("SyzygyProbeDepth must be 1-100"))
        );
        assert!(!lines.iter().any(|l| l.contains("SyzygyProbeLimit must")));
    }

    #[test]
    fn book_options() {
        let lines = session(
            "uci\nsetoption name BookFile value /no/such/book.bin\nsetoption name OwnBook value true\nposition startpos\ngo depth 2\nisready\n",
        );
        assert!(
            lines
                .iter()
                .any(|l| l == "option name OwnBook type check default false")
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("opening book not loaded: /no/such/book.bin"))
        );
        // No book loaded: it searches as usual.
        assert!(lines.iter().any(|l| l.starts_with("info depth 2")));
        assert!(lines.iter().any(|l| l.starts_with("bestmove ")));
    }

    #[test]
    fn multipv_reports_several_lines() {
        let lines =
            session("setoption name MultiPV value 3\nposition startpos\ngo depth 4\nisready\n");
        let depth2: Vec<&String> = lines
            .iter()
            .filter(|l| l.starts_with("info depth 2 "))
            .collect();
        assert_eq!(depth2.len(), 3, "{lines:?}");
        for (k, line) in depth2.iter().enumerate() {
            assert!(line.contains(&format!(" multipv {} ", k + 1)), "{line}");
        }
        // Three different first moves.
        let firsts: std::collections::HashSet<&str> = depth2
            .iter()
            .map(|l| l.split(" pv ").nth(1).unwrap().split(' ').next().unwrap())
            .collect();
        assert_eq!(firsts.len(), 3);
    }

    #[test]
    fn strength_options() {
        let lines = session(
            "setoption name Skill Level value 3\nsetoption name UCI_LimitStrength value true\nsetoption name UCI_Elo value 1500\nposition startpos\ngo movetime 200\nisready\n",
        );
        assert!(!lines.iter().any(|l| l.starts_with("info string")));
        assert!(lines.iter().any(|l| l.starts_with("bestmove ")));
        // Weak levels report one line, though they search several to choose from.
        assert!(!lines.iter().any(|l| l.contains(" multipv 2 ")));
        let bad = session(
            "setoption name UCI_Elo value 3000\nsetoption name Skill Level value 21\nisready\n",
        );
        assert!(bad.iter().any(|l| l.contains("UCI_Elo must be 1000-2400")));
        assert!(bad.iter().any(|l| l.contains("Skill Level must be 0-20")));
    }

    #[test]
    fn threads_option() {
        let lines = session(
            "uci\nsetoption name Threads value 3\nposition startpos\ngo depth 6\nisready\n",
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("option name Threads type spin default 1"))
        );
        assert!(!lines.iter().any(|l| l.starts_with("info string")));
        assert!(lines.iter().any(|l| l.starts_with("bestmove ")));
        let bad = session("setoption name Threads value 0\nisready\n");
        assert!(bad.iter().any(|l| l.contains("Threads must be 1-256")));
    }

    #[test]
    fn handles_windows_line_endings() {
        let lines = session("uci\r\nisready\r\n");
        assert_eq!(lines.last().unwrap(), "readyok");
    }

    #[test]
    fn parses_go_clock_arguments() {
        let pos = Position::startpos();
        let limits = parse_go(
            &pos,
            &[
                "wtime",
                "60000",
                "btime",
                "-5",
                "winc",
                "1000",
                "movestogo",
                "20",
            ],
        );
        assert_eq!(limits.time[0], Some(Duration::from_secs(60)));
        assert_eq!(
            limits.time[1],
            Some(Duration::ZERO),
            "negative clocks become zero"
        );
        assert_eq!(limits.increment[0], Some(Duration::from_secs(1)));
        assert_eq!(limits.moves_to_go, Some(20));
    }
}
