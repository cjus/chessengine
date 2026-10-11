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
//! ## What's here now
//!
//! - [`eval`]: material evaluation (stage 5).
//! - [`search`]: negamax with alpha-beta, mate scores and draw detection (stage 5), driven by
//!   iterative deepening in [`Engine::search`] (stage 6), with a quiescence search at the
//!   horizon (stage 7).
//! - [`ordering`]: move ordering by MVV-LVA (stage 7).
//! - [`tt`]: the transposition table (stage 8).
//! - [`time`]: time management: soft and hard limits per move (stage 6).
//! - [`bench`](mod@bench): the fixed benchmark behind `cb2026 bench` and every commit's `Bench:` line.
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

pub mod bench;
pub mod eval;
pub mod ordering;
pub mod search;
pub mod time;
pub mod tt;

use std::fmt;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use cb2026_chess::{Move, Position};

use eval::EvalParams;
use search::{DEFAULT_DEPTH, INFINITY, MATE, MAX_PLY, RootResult, Searcher, Techniques, to_score};
use time::TimeParams;
use tt::TranspositionTable;

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
    /// How full the transposition table is, in permille.
    pub hashfull: u32,
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
#[derive(Clone, Debug, Default)]
pub struct Engine {
    /// The evaluation's numbers.
    pub params: EvalParams,
    /// Which search techniques are switched on.
    pub techniques: Techniques,
    /// The time manager's settings.
    pub time: TimeParams,
    /// The transposition table, kept from one search to the next.
    pub tt: TranspositionTable,
}

impl Engine {
    /// A new engine with the default settings.
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Forgets everything learned in the previous game.
    pub fn new_game(&mut self) {
        self.tt.clear();
    }

    /// Replaces the transposition table with an empty one of about `megabytes` MB.
    pub fn set_hash_size(&mut self, megabytes: usize) {
        self.tt = TranspositionTable::new(megabytes);
    }

    // ANCHOR: search
    /// Chooses a move in `position`.
    ///
    /// `history` holds the hash keys of the earlier positions in the game. `stop` may be set by
    /// another thread to end the search early. `on_info` is called after each completed depth,
    /// and once more for a depth cut short if it found a move worth playing.
    ///
    /// The search uses **iterative deepening**: it searches to depth 1, then 2, then 3, and so
    /// on, until a limit says stop. Each depth starts with the best move of the one before,
    /// and when time runs out mid-depth the engine still has a complete answer to fall back on.
    pub fn search(
        &mut self,
        position: &Position,
        history: &[u64],
        limits: &Limits,
        stop: &AtomicBool,
        on_info: &mut dyn FnMut(&SearchInfo),
    ) -> SearchResult {
        let start = Instant::now();
        let mut root_moves: Vec<Move> = position
            .legal_moves()
            .iter()
            .copied()
            .filter(|mv| limits.search_moves.is_empty() || limits.search_moves.contains(mv))
            .collect();
        if root_moves.is_empty() {
            return SearchResult::default();
        }
        if self.techniques.mvv_lva {
            ordering::order_moves(position, &mut root_moves);
        }
        // The previous search often left a best move for this position.
        self.tt.new_search();
        if self.techniques.tt
            && let Some(mv) = self.tt.probe(position.hash()).and_then(|e| e.best_move)
            && let Some(at) = root_moves.iter().position(|&m| m == mv)
        {
            root_moves[..=at].rotate_right(1);
        }

        let budget = time::budget(limits, position.side_to_move(), &self.time);
        let unlimited = limits.depth.is_none()
            && limits.nodes.is_none()
            && limits.mate.is_none()
            && budget.hard.is_none()
            && !limits.infinite;
        let mut max_depth = limits
            .depth
            .unwrap_or(if unlimited { DEFAULT_DEPTH } else { MAX_PLY });
        if let Some(moves) = limits.mate {
            // A mate in N moves is 2N - 1 plies away.
            max_depth = max_depth.min(2 * moves);
        }
        let max_depth = max_depth.clamp(1, MAX_PLY);

        let mut searcher = Searcher::new(
            &self.params,
            &mut self.tt,
            self.techniques,
            stop,
            limits,
            history,
            budget.hard.map(|hard| start + hard),
        );
        let mut best: Option<RootResult> = None;
        for depth in 1..=max_depth {
            let result = searcher.search_root(position, &root_moves, depth);
            // A stopped iteration still counts if it finished at least one move: the first
            // move searched is the previous best, so its result is at least as well-founded.
            let usable = result.complete || result.score > -INFINITY;
            if usable {
                best = Some(result.clone());
                on_info(&SearchInfo {
                    depth,
                    seldepth: result.seldepth,
                    score: to_score(result.score),
                    nodes: result.nodes,
                    time: start.elapsed(),
                    pv: result.pv.clone(),
                    hashfull: searcher.hashfull(),
                });
            }
            if !result.complete {
                break;
            }
            // Next depth: try this depth's best move first.
            if let Some(best_move) = result.best_move {
                root_moves.retain(|&mv| mv != best_move);
                root_moves.insert(0, best_move);
            }
            // A mate found at this depth can't be beaten by searching deeper.
            if result.score.abs() >= MATE - depth as i32 {
                break;
            }
            // Starting another depth now would probably not finish in time.
            if budget.soft.is_some_and(|soft| start.elapsed() >= soft) {
                break;
            }
            if searcher.aborted() {
                break;
            }
        }

        let best = best.unwrap_or_default();
        SearchResult {
            best_move: best.best_move.or(root_moves.first().copied()),
            ponder_move: best.pv.get(1).copied(),
        }
    }
    // ANCHOR_END: search
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(pos: &Position, limits: &Limits) -> (SearchResult, Vec<SearchInfo>) {
        let stop = AtomicBool::new(false);
        let mut reports = Vec::new();
        let result = Engine::new().search(pos, &[], limits, &stop, &mut |info| {
            reports.push(info.clone())
        });
        (result, reports)
    }

    /// Searches with `engine`, returning the last report.
    fn search_with(engine: &mut Engine, fen: &str, depth: u32) -> SearchInfo {
        let stop = AtomicBool::new(false);
        let mut last = None;
        engine.search(
            &Position::from_fen(fen).unwrap(),
            &[],
            &Limits::depth(depth),
            &stop,
            &mut |info| last = Some(info.clone()),
        );
        last.unwrap()
    }

    const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

    #[test]
    fn transposition_table_saves_nodes_without_changing_the_result() {
        let with = search_with(&mut Engine::new(), KIWIPETE, 4);
        let mut engine = Engine::new();
        engine.techniques.tt = false;
        let without = search_with(&mut engine, KIWIPETE, 4);
        assert_eq!(with.score, without.score);
        assert_eq!(with.pv.first(), without.pv.first());
        assert!(
            with.nodes < without.nodes,
            "{} vs {}",
            with.nodes,
            without.nodes
        );
        assert!(with.hashfull > 0);
        assert_eq!(without.hashfull, 0);
    }

    #[test]
    fn the_table_is_kept_between_searches_and_cleared_by_new_game() {
        let mut engine = Engine::new();
        let first = search_with(&mut engine, KIWIPETE, 4);
        let again = search_with(&mut engine, KIWIPETE, 4);
        assert_eq!(again.score, first.score);
        assert!(
            again.nodes < first.nodes / 2,
            "{} vs {}",
            again.nodes,
            first.nodes
        );
        engine.new_game();
        assert_eq!(search_with(&mut engine, KIWIPETE, 4).nodes, first.nodes);
    }

    #[test]
    fn mate_scores_survive_the_table() {
        // Doubled rooks on the back rank: mate in 2, searched again from the same table.
        let fen = "2r3k1/5ppp/8/8/8/8/3R1PPP/3R2K1 w - - 0 1";
        let mut engine = Engine::new();
        for _ in 0..2 {
            assert_eq!(search_with(&mut engine, fen, 6).score, Score::Mate(2));
        }
    }

    #[test]
    fn id_name_is_name_then_version() {
        assert_eq!(id_name(), format!("cb2026 {VERSION}"));
    }

    #[test]
    fn plays_a_legal_move_and_reports_progress() {
        let pos = Position::startpos();
        let (result, reports) = search(&pos, &Limits::depth(2));
        let mv = result.best_move.expect("a move");
        assert!(pos.legal_moves().contains(&mv));
        assert_eq!(reports.len(), 2, "one report per depth");
        assert_eq!(reports[1].pv.first(), Some(&mv));
    }

    #[test]
    fn respects_search_moves() {
        let pos = Position::startpos();
        let only = pos.parse_uci_move("a2a3").unwrap();
        let limits = Limits {
            search_moves: vec![only],
            ..Limits::depth(2)
        };
        assert_eq!(search(&pos, &limits).0.best_move, Some(only));
    }

    #[test]
    fn stops_at_the_node_limit() {
        let limits = Limits {
            nodes: Some(2000),
            depth: Some(MAX_PLY),
            ..Limits::default()
        };
        let (result, reports) = search(&Position::startpos(), &limits);
        assert!(result.best_move.is_some(), "a move even when stopped early");
        assert!(reports[0].nodes < 2000 + 1024 + 64);
    }

    #[test]
    fn iterative_deepening_reports_every_depth() {
        let (_, reports) = search(&Position::startpos(), &Limits::depth(4));
        let depths: Vec<u32> = reports.iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![1, 2, 3, 4]);
        assert!(reports.windows(2).all(|w| w[0].nodes <= w[1].nodes));
    }

    #[test]
    fn respects_movetime() {
        let limits = Limits {
            movetime: Some(Duration::from_millis(200)),
            ..Limits::default()
        };
        let start = Instant::now();
        let (result, reports) = search(&Position::startpos(), &limits);
        assert!(
            start.elapsed() < Duration::from_millis(400),
            "{:?}",
            start.elapsed()
        );
        // The last report, even for a depth cut short, describes the move played.
        assert_eq!(
            reports.last().unwrap().pv.first(),
            result.best_move.as_ref()
        );
    }

    #[test]
    fn stops_when_it_finds_a_mate() {
        // Mate in one: there's no need to search to depth 30.
        let pos = Position::from_fen("6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1").unwrap();
        let (result, reports) = search(&pos, &Limits::depth(30));
        assert_eq!(result.best_move.unwrap().to_string(), "e1e8");
        assert!(reports.last().unwrap().depth <= 2);
        assert_eq!(reports.last().unwrap().score, Score::Mate(1));
    }

    #[test]
    fn go_mate_limits_the_depth() {
        let limits = Limits {
            mate: Some(1),
            ..Limits::default()
        };
        let (_, reports) = search(&Position::startpos(), &limits);
        assert_eq!(reports.last().unwrap().depth, 2);
    }

    #[test]
    fn no_move_when_mated() {
        let mated =
            Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
                .unwrap();
        let (result, reports) = search(&mated, &Limits::default());
        assert_eq!(result.best_move, None);
        assert!(reports.is_empty());
    }

    #[test]
    fn scores_display_uci_style() {
        assert_eq!(Score::Centipawns(-31).to_string(), "cp -31");
        assert_eq!(Score::Mate(3).to_string(), "mate 3");
    }
}
