//! # cb2026-engine: choosing a move
//!
//! Given a position from `cb2026-chess`, this crate decides what to play: it searches the game
//! tree and evaluates the positions it reaches. It does no I/O; the protocols and the shell
//! drive it through the Engine API and receive search progress through a callback.
//!
//! ## The Engine API
//!
//! ```
//! use std::sync::atomic::AtomicBool;
//! use cb2026_chess::Position;
//! use cb2026_engine::{Engine, Limits};
//!
//! let mut engine = Engine::new();
//! let stop = AtomicBool::new(false);
//! let result = engine.search(&Position::startpos(), &[], &Limits::depth(1), &stop, &mut |info| {
//!     println!("depth {} best {:?}", info.depth, info.pv.first());
//! });
//! assert!(result.best_move.is_some());
//! ```
//!
//! - [`Limits`] says when to stop: a depth, a node count, a fixed time, the clock, or never
//!   (`infinite`, until the caller sets the stop flag).
//! - The **stop flag** (`AtomicBool`) lets another thread end the search at any moment; the
//!   protocols set it on `stop` and `quit`.
//! - The **info callback** receives a [`SearchInfo`] as the search makes progress (depth,
//!   score, nodes, principal variation); UCI turns it into `info` lines and the shell into its
//!   thinking display.
//! - The **history** argument holds the hash keys of the positions before this one in the
//!   game, for spotting repetitions.
//!
//! ## What the "search" is right now
//!
//! Stage 4 has no search: the engine plays a **random legal move**. That is enough to test
//! the whole path from a GUI to the engine and back. Stage 5 brings material evaluation and
//! alpha-beta search.
//!
//! ## What will live here
//!
//! - **Search (stages 5–14):** negamax alpha-beta, iterative deepening, quiescence search, a
//!   transposition table, move ordering, principal variation search, and the pruning and
//!   reduction techniques modern engines use. Each technique can be switched off at runtime so
//!   its value can be measured.
//! - **Evaluation (stages 9, 15–16):** a tapered, hand-crafted evaluation whose weights are tuned
//!   on self-play games, with a per-term trace for the shell's `eval` command.
//! - **Everything else a real engine needs (stages 6, 17–19):** time management, Lazy SMP,
//!   Syzygy tablebases, a Polyglot opening book, and strength limiting.
//!
//! ## `unsafe` policy
//!
//! This crate denies `unsafe` code rather than forbidding it, because two later modules may need
//! it: SIMD evaluation and the lock-free transposition table. Each such module will opt in with
//! `#[allow(unsafe_code)]` and justify every `unsafe` block with a `// SAFETY:` comment.

#![deny(unsafe_code)]

use std::fmt;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use cb2026_chess::{Move, Position};

/// The engine's name, as reported to GUIs (`id name` in UCI, `myname` in XBoard).
pub const NAME: &str = "cb2026";

/// The engine's version, taken from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The engine's authors, as reported to GUIs (`id author` in UCI).
pub const AUTHOR: &str = "the cb2026 authors";

/// The full engine identification, for example `cb2026 0.1.0`.
///
/// GUIs show this string in their engine lists, so it includes the version: testers need to
/// know exactly which build played a game.
pub fn id_name() -> String {
    format!("{NAME} {VERSION}")
}

/// When the search should stop. Every field is optional; with none set, the search decides
/// for itself (for now, it simply plays its move).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    /// Search this many plies deep.
    pub depth: Option<u32>,
    /// Search about this many positions.
    pub nodes: Option<u64>,
    /// Spend this long, exactly.
    pub movetime: Option<Duration>,
    /// Time left on White's and Black's clocks.
    pub time: [Option<Duration>; 2],
    /// Increment per move for White and Black.
    pub increment: [Option<Duration>; 2],
    /// Moves until the next time control, if the clock isn't sudden death.
    pub moves_to_go: Option<u32>,
    /// Look for a mate in this many moves.
    pub mate: Option<u32>,
    /// Search until told to stop.
    pub infinite: bool,
    /// Only consider these moves at the root (empty means all).
    pub search_moves: Vec<Move>,
}

impl Limits {
    /// Limits that stop at `depth` plies.
    pub fn depth(depth: u32) -> Limits {
        Limits {
            depth: Some(depth),
            ..Limits::default()
        }
    }
}

/// A search score.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Score {
    /// Centipawns (hundredths of a pawn) from the side to move's point of view.
    Centipawns(i32),
    /// Mate in this many moves (negative: the side to move gets mated).
    Mate(i32),
}

impl fmt::Display for Score {
    /// UCI style: `cp 31` or `mate -3`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Score::Centipawns(cp) => write!(f, "cp {cp}"),
            Score::Mate(moves) => write!(f, "mate {moves}"),
        }
    }
}

/// Progress reported during a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchInfo {
    /// The depth just completed.
    pub depth: u32,
    /// The deepest ply any line reached (with extensions and quiescence).
    pub seldepth: u32,
    /// The score of the best line.
    pub score: Score,
    /// Positions searched so far.
    pub nodes: u64,
    /// Time spent so far.
    pub time: Duration,
    /// The principal variation: the best line found, starting with the best move.
    pub pv: Vec<Move>,
}

/// The outcome of a search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchResult {
    /// The move to play, or `None` if there is no legal move (checkmate or stalemate).
    pub best_move: Option<Move>,
    /// The expected reply, for pondering.
    pub ponder_move: Option<Move>,
}

/// The engine: its settings and everything it remembers between searches.
#[derive(Clone, Debug)]
pub struct Engine {
    rng: u64,
}

impl Default for Engine {
    fn default() -> Engine {
        Engine::new()
    }
}

impl Engine {
    /// A new engine, seeded from the clock so that games vary.
    pub fn new() -> Engine {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x2026, |d| d.as_nanos() as u64);
        Engine::with_seed(seed)
    }

    /// A new engine whose random choices are fixed by `seed` (for reproducible tests).
    pub fn with_seed(seed: u64) -> Engine {
        Engine { rng: seed | 1 }
    }

    /// Forgets everything learned in the previous game.
    pub fn new_game(&mut self) {}

    /// A pseudo-random number (xorshift64).
    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    // ANCHOR: search
    /// Chooses a move in `position`.
    ///
    /// `history` holds the hash keys of the earlier positions in the game. `stop` may be set by
    /// another thread to end the search early. `on_info` is called with progress reports.
    pub fn search(
        &mut self,
        position: &Position,
        _history: &[u64],
        limits: &Limits,
        _stop: &AtomicBool,
        on_info: &mut dyn FnMut(&SearchInfo),
    ) -> SearchResult {
        let start = Instant::now();
        let legal = position.legal_moves();
        let candidates: Vec<Move> = legal
            .iter()
            .copied()
            .filter(|mv| limits.search_moves.is_empty() || limits.search_moves.contains(mv))
            .collect();
        // Stage 4: no search yet, just a random legal move.
        let best_move = match candidates.len() {
            0 => None,
            n => Some(candidates[(self.random() % n as u64) as usize]),
        };
        if let Some(mv) = best_move {
            on_info(&SearchInfo {
                depth: 1,
                seldepth: 1,
                score: Score::Centipawns(0),
                nodes: candidates.len() as u64,
                time: start.elapsed(),
                pv: vec![mv],
            });
        }
        SearchResult {
            best_move,
            ponder_move: None,
        }
    }
    // ANCHOR_END: search
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(engine: &mut Engine, pos: &Position, limits: &Limits) -> (SearchResult, usize) {
        let stop = AtomicBool::new(false);
        let mut reports = 0;
        let result = engine.search(pos, &[], limits, &stop, &mut |_| reports += 1);
        (result, reports)
    }

    #[test]
    fn id_name_is_name_then_version() {
        assert_eq!(id_name(), format!("cb2026 {VERSION}"));
    }

    #[test]
    fn plays_a_legal_move_and_reports_progress() {
        let mut engine = Engine::with_seed(7);
        let pos = Position::startpos();
        let (result, reports) = search(&mut engine, &pos, &Limits::depth(1));
        let mv = result.best_move.expect("a move");
        assert!(pos.legal_moves().contains(&mv));
        assert_eq!(reports, 1);
    }

    #[test]
    fn same_seed_same_moves() {
        let pos = Position::startpos();
        let a = search(&mut Engine::with_seed(42), &pos, &Limits::default()).0;
        let b = search(&mut Engine::with_seed(42), &pos, &Limits::default()).0;
        assert_eq!(a, b);
    }

    #[test]
    fn respects_search_moves() {
        let pos = Position::startpos();
        let only = pos.parse_uci_move("a2a3").unwrap();
        let limits = Limits {
            search_moves: vec![only],
            ..Limits::default()
        };
        for seed in 1..20 {
            let result = search(&mut Engine::with_seed(seed), &pos, &limits).0;
            assert_eq!(result.best_move, Some(only));
        }
    }

    #[test]
    fn no_move_when_mated() {
        let mated =
            Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
                .unwrap();
        let (result, reports) = search(&mut Engine::with_seed(1), &mated, &Limits::default());
        assert_eq!(result.best_move, None);
        assert_eq!(reports, 0);
    }

    #[test]
    fn scores_display_uci_style() {
        assert_eq!(Score::Centipawns(-31).to_string(), "cp -31");
        assert_eq!(Score::Mate(3).to_string(), "mate 3");
    }
}
