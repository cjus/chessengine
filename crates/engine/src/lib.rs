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
//! - [`see`]: static exchange evaluation (stage 14).
//! - [`hce`]: the full hand-crafted evaluation's positional terms (stage 15).
//! - [`tuning`]: the evaluation as features times parameters, for tuning (stage 16).
//! - [`tuned`]: the parameter values found by Texel tuning (stage 16, generated), available as
//!   `EvalParams::tuned()`.
//! - [`tablebase`]: endgame tablebase probing through a trait (stage 18).
//! - [`book`]: Polyglot opening books (stage 18).
//! - [`levels`]: strength levels (stage 19).
//! - [`time`]: time management: soft and hard limits per move (stage 6).
//! - [`bench`](mod@bench): the fixed benchmark behind `cb2026 bench` and every commit's `Bench:` line.
//!
//! ## What will live here
//!
//! - **Search (stages 5–14):** negamax alpha-beta, iterative deepening, quiescence search, a
//!   transposition table, move ordering, principal variation search, and the pruning and
//!   reduction techniques modern engines use. Each technique can be switched off at runtime so
//!   its value can be measured.
//! - **Evaluation (stages 9, 15–16):** a tapered, hand-crafted evaluation with a per-term trace
//!   for the shell's `eval` command, and a Texel tuner that fits its weights to self-play games
//!   (the tuned values are available, but haven't yet measured stronger than the hand-set ones).
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
pub mod book;
mod book_keys;
pub mod eval;
pub mod hce;
pub mod levels;
pub mod ordering;
pub mod search;
pub mod see;
pub mod stats;
pub mod tablebase;
pub mod time;
pub mod tt;
pub mod tuned;
pub mod tuning;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use cb2026_chess::{Move, Position};

use book::OpeningBook;
use eval::EvalParams;
use ordering::OrderingTables;
use search::{
    DEFAULT_DEPTH, INFINITY, MATE, MAX_PLY, RootResult, SearchParams, Searcher, StopConditions,
    Techniques, to_score,
};
use stats::{SearchStats, SearchTree};
use tablebase::{Tablebase, TablebaseLoader, TablebaseParams};
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
    /// Tablebase probes that gave a result (main thread).
    pub tbhits: u64,
    /// Which line this is, from 1, when searching several (MultiPV).
    pub multipv: u32,
}

/// The outcome of a search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchResult {
    /// The move to play, or `None` if there is no legal move (checkmate or stalemate).
    pub best_move: Option<Move>,
    /// The expected reply, for pondering.
    pub ponder_move: Option<Move>,
    /// What the search did, all threads together (see [`stats`]).
    pub stats: SearchStats,
}

/// The most search threads the engine accepts (the UCI `Threads` option's maximum).
pub const MAX_THREADS: usize = 256;

/// The engine: its settings and everything it remembers between searches.
#[derive(Clone, Debug)]
pub struct Engine {
    /// The evaluation's numbers.
    pub params: EvalParams,
    /// Which search techniques are switched on.
    pub techniques: Techniques,
    /// The time manager's settings.
    pub time: TimeParams,
    /// The search's tunable numbers.
    pub search_params: SearchParams,
    /// The transposition table, kept from one search to the next.
    pub tt: TranspositionTable,
    /// Killer moves, countermoves and history, for ordering quiet moves.
    pub ordering: OrderingTables,
    /// Search threads (Lazy SMP, stage 17): 1 means a single-threaded search.
    pub threads: usize,
    /// Endgame tablebases, if any are loaded (stage 18).
    pub tablebase: Option<Arc<dyn Tablebase>>,
    /// When to probe them.
    pub tablebase_params: TablebaseParams,
    /// How to open tablebase files, if the program embedding the engine supports it.
    pub tablebase_loader: Option<TablebaseLoader>,
    /// An opening book, if one is loaded (stage 18).
    pub book: Option<Arc<OpeningBook>>,
    /// Play from the book when it has a move (the UCI `OwnBook` option; off by default).
    pub own_book: bool,
    /// How many best lines to search and report (the UCI `MultiPV` option).
    pub multipv: usize,
    /// The strength level, 0 to [`levels::FULL_STRENGTH`] (the UCI `Skill Level`).
    pub skill_level: u32,
    /// Limit the strength to [`Engine::elo`] (the UCI `UCI_LimitStrength`).
    pub limit_strength: bool,
    /// The strength to play at when limited (the UCI `UCI_Elo`).
    pub elo: u32,
}

impl Default for Engine {
    fn default() -> Engine {
        Engine {
            params: EvalParams::default(),
            techniques: Techniques::default(),
            time: TimeParams::default(),
            search_params: SearchParams::default(),
            tt: TranspositionTable::default(),
            ordering: OrderingTables::default(),
            threads: 1,
            tablebase: None,
            tablebase_params: TablebaseParams::default(),
            tablebase_loader: None,
            book: None,
            own_book: false,
            multipv: 1,
            skill_level: levels::FULL_STRENGTH,
            limit_strength: false,
            elo: levels::ELO_RANGE.1,
        }
    }
}

impl Engine {
    /// A new engine with the default settings.
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Forgets everything learned in the previous game.
    pub fn new_game(&mut self) {
        self.tt.clear();
        self.ordering.clear();
    }

    /// Opens the tablebases found at `path` (the UCI `SyzygyPath`): directories separated by
    /// `;` (or `:` outside Windows). An empty path unloads them. Returns a summary.
    pub fn load_tablebases(&mut self, path: &str) -> Result<String, String> {
        if path.trim().is_empty() || path == "<empty>" {
            self.tablebase = None;
            return Ok("No tablebases".to_string());
        }
        let loader = self
            .tablebase_loader
            .ok_or("this build has no tablebase support")?;
        let tb = loader(path)?;
        let summary = format!("Tablebases for up to {} pieces", tb.max_pieces());
        self.tablebase = Some(tb);
        Ok(summary)
    }

    /// The level the engine plays at: [`Engine::skill_level`], or the level for
    /// [`Engine::elo`] when the strength is limited, whichever is weaker.
    pub fn level(&self) -> u32 {
        let skill = self.skill_level.min(levels::FULL_STRENGTH);
        if self.limit_strength {
            skill.min(levels::level_for_elo(self.elo))
        } else {
            skill
        }
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

        // ANCHOR: book
        // The opening book: play its move at once (unless asked to analyse).
        if self.own_book
            && !limits.infinite
            && limits.search_moves.is_empty()
            && let Some(book) = &self.book
        {
            // The dice: the clock's nanoseconds, mixed with the position.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let random = u64::from(nanos) ^ position.hash();
            if let Some(mv) = book.pick(position, random)
                && root_moves.contains(&mv)
            {
                return SearchResult {
                    best_move: Some(mv),
                    ..SearchResult::default()
                };
            }
        }
        // ANCHOR_END: book
        if self.techniques.mvv_lva {
            ordering::order_moves(position, &mut root_moves);
        }
        // The previous search often left a best move for this position.
        self.tt.new_search();
        self.ordering.new_search();
        if self.techniques.tt
            && let Some(mv) = self.tt.probe(position.hash()).and_then(|e| e.best_move)
            && let Some(at) = root_moves.iter().position(|&m| m == mv)
        {
            root_moves[..=at].rotate_right(1);
        }

        // ANCHOR: tb_root
        // In a tablebase position, keep only the move the tables recommend: it keeps the best
        // result and makes progress under the fifty-move rule. The search still runs (and
        // reports), but can only choose that move.
        if let Some(tb) = &self.tablebase
            && tablebase::covers(tb.as_ref(), &self.tablebase_params, position)
            && let Some((_, mv)) = tb.probe_root(position)
            && root_moves.contains(&mv)
        {
            root_moves = vec![mv];
        }
        // ANCHOR_END: tb_root

        // ANCHOR: level_limits
        // A weaker level searches fewer nodes and several lines to choose from.
        let level = levels::settings(self.level());
        let mut level_limits = limits.clone();
        if let Some(nodes) = level.nodes {
            level_limits.nodes = Some(limits.nodes.map_or(nodes, |n| n.min(nodes)));
        }
        let limits = &level_limits;
        let lines = self.multipv.max(level.candidates).max(1);
        // ANCHOR_END: level_limits

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

        let deadline = budget.hard.map(|hard| start + hard);
        let root_fallback = root_moves.first().copied();
        // Lazy SMP (stage 17): helper threads search the same position, sharing only the
        // transposition table. They stop when the main thread is done.
        let helpers_stop = AtomicBool::new(false);
        let helper_nodes = AtomicU64::new(0);
        let Engine {
            params,
            techniques,
            search_params,
            tt,
            ordering,
            threads,
            tablebase,
            tablebase_params,
            multipv,
            ..
        } = self;
        let tablebase = tablebase.as_deref();
        let self_multipv = (*multipv).max(1);
        let tablebase_params = *tablebase_params;
        let (params, techniques, search_params, tt) = (&*params, *techniques, *search_params, &*tt);
        thread::scope(|scope| {
            let mut helper_threads = Vec::new();
            for helper in 1..*threads {
                let root_moves = root_moves.clone();
                let (helpers_stop, helper_nodes) = (&helpers_stop, &helper_nodes);
                helper_threads.push(scope.spawn(move || {
                    let mut ordering = OrderingTables::default();
                    let mut searcher = Searcher::new(
                        params,
                        search_params,
                        tt,
                        &mut ordering,
                        techniques,
                        StopConditions {
                            stop: helpers_stop,
                            deadline,
                            max_nodes: None,
                        },
                        history,
                    );
                    searcher.count_nodes_in(helper_nodes);
                    if let Some(tb) = tablebase {
                        searcher.use_tablebase(tb, tablebase_params);
                    }
                    run_helper(&mut searcher, position, root_moves, max_depth, helper);
                    searcher.stats()
                }));
            }
            let mut searcher = Searcher::new(
                params,
                search_params,
                tt,
                ordering,
                techniques,
                StopConditions {
                    stop,
                    deadline,
                    max_nodes: limits.nodes,
                },
                history,
            );
            if let Some(tb) = tablebase {
                searcher.use_tablebase(tb, tablebase_params);
            }
            let result = main_search(
                &mut searcher,
                MainSearch {
                    position,
                    root_moves,
                    max_depth,
                    start,
                    soft: budget.soft,
                    helper_nodes: &helper_nodes,
                    lines,
                    reported: self_multipv,
                },
                on_info,
            );
            helpers_stop.store(true, Ordering::Relaxed);
            let mut stats = searcher.stats();
            for helper in helper_threads {
                if let Ok(helper_stats) = helper.join() {
                    stats.add(&helper_stats);
                }
            }
            // ANCHOR: level_choice
            let Some(best) = result.first() else {
                return SearchResult {
                    best_move: root_fallback,
                    ponder_move: None,
                    stats,
                };
            };
            // At full strength, the best line; at a lower level, one of the candidates.
            let candidates: Vec<(Move, i32)> = result
                .iter()
                .filter_map(|line| line.best_move.map(|mv| (mv, line.score)))
                .collect();
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let random = (u64::from(nanos) ^ position.hash()).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let chosen = levels::choose(&candidates, &level, random).or(best.best_move);
            let line = result
                .iter()
                .find(|line| line.best_move == chosen)
                .unwrap_or(best);
            let best_move = chosen.or(root_fallback);
            // The expected reply is the line's second move. A line cut short (by a
            // transposition table cutoff just below the root) may not have one; then the
            // table's best move for the position after ours is the next best guess.
            let ponder_move = line.pv.get(1).copied().or_else(|| {
                let mut after = position.clone();
                after.make_move(best_move?);
                tt.probe(after.hash())
                    .and_then(|entry| entry.best_move)
                    .filter(|mv| after.legal_moves().contains(mv))
            });
            SearchResult {
                best_move,
                ponder_move,
                stats,
            }
            // ANCHOR_END: level_choice
        })
    }
    // ANCHOR_END: search

    /// Searches `position` to `depth` on one thread and records the search tree of the last
    /// iteration, for nodes up to `max_ply` plies from the root (at most `max_events`
    /// decisions; see [`stats`]). The earlier iterations run first, unrecorded, so the move
    /// ordering is what a real search would have. Returns the tree and the iteration's result.
    pub fn search_tree(
        &mut self,
        position: &Position,
        history: &[u64],
        depth: u32,
        max_ply: usize,
        max_events: usize,
    ) -> (SearchTree, SearchInfo) {
        let start = Instant::now();
        let stop = AtomicBool::new(false);
        let mut root_moves: Vec<Move> = position.legal_moves().iter().copied().collect();
        if self.techniques.mvv_lva {
            ordering::order_moves(position, &mut root_moves);
        }
        self.tt.new_search();
        self.ordering.new_search();
        let mut searcher = Searcher::new(
            &self.params,
            self.search_params,
            &self.tt,
            &mut self.ordering,
            self.techniques,
            StopConditions {
                stop: &stop,
                deadline: None,
                max_nodes: None,
            },
            history,
        );
        if let Some(tb) = self.tablebase.as_deref() {
            searcher.use_tablebase(tb, self.tablebase_params);
        }
        let depth = depth.clamp(1, MAX_PLY);
        let mut previous = None;
        for d in 1..depth {
            let result = searcher.search_depth(position, &mut root_moves, d, previous);
            previous = Some(result.score);
            if let Some(mv) = result.best_move {
                root_moves.retain(|&m| m != mv);
                root_moves.insert(0, mv);
            }
        }
        searcher.record_tree(SearchTree::new(max_ply, max_events));
        let result = searcher.search_depth(position, &mut root_moves, depth, previous);
        let info = SearchInfo {
            depth,
            seldepth: result.seldepth,
            score: to_score(result.score),
            nodes: result.nodes,
            time: start.elapsed(),
            pv: result.pv,
            hashfull: searcher.hashfull(),
            tbhits: searcher.tb_hits(),
            multipv: 1,
        };
        (searcher.take_tree().unwrap_or_default(), info)
    }
}

/// What the main search thread needs besides its [`Searcher`].
struct MainSearch<'a> {
    position: &'a Position,
    root_moves: Vec<Move>,
    max_depth: u32,
    start: Instant,
    soft: Option<Duration>,
    helper_nodes: &'a AtomicU64,
    /// How many lines to search (MultiPV, or a level's candidates).
    lines: usize,
    /// How many of them to report.
    reported: usize,
}

// ANCHOR: main_search
/// The main thread's iterative deepening: it decides when to stop, reports progress, and
/// returns the best lines of the last depth it searched, best first.
///
/// With several lines (MultiPV), each depth searches the root once per line, leaving out the
/// moves already chosen as better lines at that depth.
fn main_search(
    searcher: &mut Searcher<'_>,
    job: MainSearch<'_>,
    on_info: &mut dyn FnMut(&SearchInfo),
) -> Vec<RootResult> {
    let MainSearch {
        position,
        mut root_moves,
        max_depth,
        start,
        soft,
        helper_nodes,
        lines: wanted,
        reported,
    } = job;
    let wanted = wanted.min(root_moves.len()).max(1);
    let mut lines: Vec<RootResult> = Vec::new();
    for depth in 1..=max_depth {
        let mut this_depth: Vec<RootResult> = Vec::new();
        let mut complete = true;
        for k in 0..wanted {
            let mut moves: Vec<Move> = root_moves
                .iter()
                .copied()
                .filter(|&mv| !this_depth.iter().any(|line| line.best_move == Some(mv)))
                .collect();
            let previous = lines.get(k).map(|line| line.score);
            let result = searcher.search_depth(position, &mut moves, depth, previous);
            // A stopped iteration still counts if it finished at least one move: the first
            // move searched is the previous best, so its result is at least as well-founded.
            if result.complete || result.score > -INFINITY {
                this_depth.push(result.clone());
            }
            if !result.complete {
                complete = false;
                break;
            }
        }
        // A later line can score higher than an earlier one (it is searched with its own
        // window, and the transposition table has learned more by then), so the lines are
        // sorted, best first, before they are reported. The sort is stable: equal scores keep
        // the order they were found in.
        this_depth.sort_by_key(|line| std::cmp::Reverse(line.score));
        for (k, line) in this_depth.iter().enumerate().take(reported) {
            on_info(&SearchInfo {
                depth,
                seldepth: line.seldepth,
                score: to_score(line.score),
                nodes: line.nodes + helper_nodes.load(Ordering::Relaxed),
                time: start.elapsed(),
                pv: line.pv.clone(),
                hashfull: searcher.hashfull(),
                tbhits: searcher.tb_hits(),
                multipv: k as u32 + 1,
            });
        }
        if !this_depth.is_empty() {
            lines = this_depth;
        }
        if !complete {
            break;
        }
        // Next depth: the lines' moves first, best first.
        for line in lines.iter().rev() {
            if let Some(mv) = line.best_move {
                root_moves.retain(|&m| m != mv);
                root_moves.insert(0, mv);
            }
        }
        // A mate found at this depth can't be beaten by searching deeper.
        if wanted == 1 && lines[0].score.abs() >= MATE - depth as i32 {
            break;
        }
        // Starting another depth now would probably not finish in time.
        if soft.is_some_and(|soft| start.elapsed() >= soft) {
            break;
        }
        if searcher.aborted() {
            break;
        }
    }
    lines
}
// ANCHOR_END: main_search

// ANCHOR: helper
/// A helper thread's search: iterative deepening on the same position until told to stop.
/// Its results are only used through the transposition table. Odd-numbered helpers start
/// one ply deeper, so the threads are less often searching the same depth at the same time.
fn run_helper(
    searcher: &mut Searcher<'_>,
    position: &Position,
    mut root_moves: Vec<Move>,
    max_depth: u32,
    helper: usize,
) {
    let mut previous = None;
    let first = 1 + (helper % 2) as u32;
    for depth in first..=max_depth {
        let result = searcher.search_depth(position, &mut root_moves, depth, previous);
        if !result.complete {
            break;
        }
        previous = Some(result.score);
        if let Some(best_move) = result.best_move {
            root_moves.retain(|&mv| mv != best_move);
            root_moves.insert(0, best_move);
        }
    }
}
// ANCHOR_END: helper

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
    fn check_extensions_see_further_along_checks() {
        // Philidor's legacy: 1. Nf7+ Kg8 2. Nh6+ Kh8 3. Qg8+ Rxg8 4. Nf7#, every move a check.
        let fen = "4r2k/6pp/8/3Q2N1/8/8/6PP/6K1 w - - 0 1";
        // Internal iterative reductions are off: they shorten some of these lines too, and
        // the test is about the extension alone.
        let mut engine = Engine::new();
        engine.techniques.check_extension = true;
        engine.techniques.iir = false;
        assert_eq!(search_with(&mut engine, fen, 7).score, Score::Mate(4));
        let mut without = engine.clone();
        without.new_game();
        without.techniques.check_extension = false;
        assert!(matches!(
            search_with(&mut without, fen, 7).score,
            Score::Centipawns(_)
        ));
    }

    #[test]
    fn internal_iterative_reductions_save_nodes_and_still_find_mates() {
        let nodes = |iir: bool| {
            let mut engine = Engine::new();
            engine.techniques.iir = iir;
            search_with(&mut engine, KIWIPETE, 8).nodes
        };
        let (with, without) = (nodes(true), nodes(false));
        assert!(with < without, "{with} vs {without}");
        let mut engine = Engine::new();
        let mate_in_two = "r2qkb1r/pp2nppp/3p4/2pNN1B1/2BnP3/3P4/PPP2PPP/R2bK2R w KQkq - 1 0";
        assert_eq!(
            search_with(&mut engine, mate_in_two, 6).score,
            Score::Mate(2)
        );
    }

    #[test]
    fn the_improving_heuristic_changes_the_search_and_still_finds_mates() {
        let nodes = |improving: bool| {
            let mut engine = Engine::new();
            engine.techniques.improving = improving;
            search_with(&mut engine, KIWIPETE, 8).nodes
        };
        assert_ne!(nodes(true), nodes(false));
        let mut engine = Engine::new();
        engine.techniques.improving = true;
        let mate_in_two = "r2qkb1r/pp2nppp/3p4/2pNN1B1/2BnP3/3P4/PPP2PPP/R2bK2R w KQkq - 1 0";
        assert_eq!(
            search_with(&mut engine, mate_in_two, 6).score,
            Score::Mate(2)
        );
    }

    #[test]
    fn statistics_add_up_to_the_nodes_searched() {
        let pos = Position::from_fen(KIWIPETE).unwrap();
        let (result, reports) = search(&pos, &Limits::depth(6));
        let stats = result.stats;
        assert_eq!(
            stats.nodes + stats.qnodes,
            reports.last().unwrap().nodes,
            "{stats:?}"
        );
        assert!(stats.cutoffs > 0 && stats.first_move_cutoffs <= stats.cutoffs);
        assert!(stats.tt_cutoffs <= stats.tt_hits);
        assert!(stats.null_move_cutoffs <= stats.null_moves);
        assert!(stats.reduction_researches <= stats.reduced);
    }

    #[test]
    fn search_trees_record_the_last_iteration() {
        let pos = Position::startpos();
        let mut engine = Engine::new();
        let (tree, info) = engine.search_tree(&pos, &[], 3, 1, 10_000);
        assert_eq!(info.depth, 3);
        assert!(!tree.truncated);
        // Every root move is entered at depth 2, and nothing deeper is recorded.
        let entered = tree
            .events
            .iter()
            .filter(|e| matches!(e.note, stats::Note::Enter { depth: 2, .. }))
            .count();
        assert!(entered >= pos.legal_moves().len(), "{entered}");
        assert!(tree.events.iter().all(|e| e.path.len() <= 1));
        // The same search without recording finds the same score.
        let (_, reports) = search(&pos, &Limits::depth(3));
        assert_eq!(reports.last().unwrap().score, info.score);
    }

    #[test]
    fn transposition_table_saves_nodes_without_changing_the_result() {
        let mut engine = Engine::new();
        engine.techniques = Techniques::sound();
        let with = search_with(&mut engine, KIWIPETE, 4);
        let mut engine = Engine::new();
        engine.techniques = Techniques::sound();
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
    fn lazy_smp_finds_the_same_mate_and_counts_every_thread() {
        let mut engine = Engine::new();
        engine.threads = 4;
        let pos = Position::from_fen("2r3k1/5ppp/8/8/8/8/3R1PPP/3R2K1 w - - 0 1").unwrap();
        let info = search_with(&mut engine, &pos.to_fen(), 6);
        assert_eq!(info.score, Score::Mate(2));
        // A deeper search: the helpers' nodes show up in the total.
        let mut single = Engine::new();
        let one = search_with(&mut single, KIWIPETE, 8);
        let four = search_with(&mut engine, KIWIPETE, 8);
        assert!(!one.pv.is_empty() && !four.pv.is_empty());
        assert!(four.nodes > 0);
    }

    #[test]
    fn helpers_stop_when_the_main_thread_does() {
        let mut engine = Engine::new();
        engine.threads = 3;
        let limits = Limits {
            movetime: Some(Duration::from_millis(150)),
            ..Limits::default()
        };
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        let result = engine.search(&Position::startpos(), &[], &limits, &stop, &mut |_| {});
        assert!(result.best_move.is_some());
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn tablebase_results_are_used_in_the_search() {
        use crate::tablebase::{TB_WIN, tests::ToyTablebase};
        // Taking the rook (Kxe2 or Qxe2) leaves king and queen against king: a tablebase win.
        let fen = "4k3/8/8/8/8/8/4r3/3QK3 w - - 0 1";
        let mut engine = Engine::new();
        engine.tablebase = Some(Arc::new(ToyTablebase));
        let info = search_with(&mut engine, fen, 4);
        assert!(info.tbhits > 0);
        let first = info.pv.first().unwrap().to_string();
        assert!(first == "e1e2" || first == "d1e2", "{first}");
        match info.score {
            Score::Centipawns(cp) => assert!(cp > TB_WIN - MAX_PLY as i32, "{cp}"),
            other => panic!("{other:?}"),
        }
        // Without tablebases: no hits.
        assert_eq!(search_with(&mut Engine::new(), fen, 4).tbhits, 0);
    }

    #[test]
    fn root_in_a_tablebase_position_plays_the_table_move() {
        use crate::tablebase::tests::ToyTablebase;
        let fen = "8/8/8/4k3/8/8/8/3QK3 w - - 0 1";
        let pos = Position::from_fen(fen).unwrap();
        let mut engine = Engine::new();
        engine.tablebase = Some(Arc::new(ToyTablebase));
        let stop = AtomicBool::new(false);
        let result = engine.search(&pos, &[], &Limits::depth(3), &stop, &mut |_| {});
        // The toy's "best move" is the first legal move.
        assert_eq!(result.best_move, pos.legal_moves().first().copied());
    }

    #[test]
    fn loading_tablebases_needs_a_loader() {
        let mut engine = Engine::new();
        assert!(engine.load_tablebases("/some/path").is_err());
        assert!(engine.load_tablebases("").is_ok());
        fn toy(_: &str) -> Result<Arc<dyn Tablebase>, String> {
            Ok(Arc::new(crate::tablebase::tests::ToyTablebase))
        }
        engine.tablebase_loader = Some(toy);
        assert_eq!(
            engine.load_tablebases("/tables").unwrap(),
            "Tablebases for up to 3 pieces"
        );
        assert!(engine.tablebase.is_some());
        engine.load_tablebases("<empty>").unwrap();
        assert!(engine.tablebase.is_none());
    }

    #[test]
    fn plays_from_the_book_only_when_asked() {
        // A one-entry book: 1. d4 from the start.
        let start = Position::startpos();
        let mut bytes = book::polyglot_key(&start).to_be_bytes().to_vec();
        let d4 = (cb2026_chess::Square::D2.index() << 6 | cb2026_chess::Square::D4.index()) as u16;
        bytes.extend(d4.to_be_bytes());
        bytes.extend(1u16.to_be_bytes());
        bytes.extend([0; 4]);
        let book = OpeningBook::from_bytes(&bytes).unwrap();
        let mut engine = Engine::new();
        engine.book = Some(Arc::new(book));
        let stop = AtomicBool::new(false);
        let mut reports = 0;
        // OwnBook off: a normal search.
        engine.search(&start, &[], &Limits::depth(2), &stop, &mut |_| reports += 1);
        assert!(reports > 0);
        engine.own_book = true;
        let mut reports = 0;
        let result = engine.search(&start, &[], &Limits::depth(2), &stop, &mut |_| reports += 1);
        assert_eq!(result.best_move.unwrap().to_string(), "d2d4");
        assert_eq!(reports, 0);
        // Out of book: searches as usual.
        let after =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - 0 1")
                .unwrap();
        let result = engine.search(&after, &[], &Limits::depth(2), &stop, &mut |_| {});
        assert!(result.best_move.is_some());
    }

    #[test]
    fn weak_levels_search_few_nodes_and_vary_their_moves() {
        let mut engine = Engine::new();
        engine.skill_level = 0;
        let pos = Position::startpos();
        let stop = AtomicBool::new(false);
        let mut chosen = std::collections::HashSet::new();
        for _ in 0..12 {
            let mut nodes = 0;
            let result = engine.search(&pos, &[], &Limits::default(), &stop, &mut |info| {
                nodes = nodes.max(info.nodes)
            });
            assert!(nodes <= 400, "{nodes}");
            chosen.insert(result.best_move.unwrap());
            engine.new_game();
        }
        // Level 0 picks among four candidates with plenty of randomness.
        assert!(chosen.len() > 1, "{chosen:?}");
        // UCI_Elo maps onto a level, and never makes the engine stronger than Skill Level.
        engine.skill_level = 20;
        engine.limit_strength = true;
        engine.elo = 1000;
        assert_eq!(engine.level(), 0);
        engine.elo = 2400;
        engine.skill_level = 5;
        assert_eq!(engine.level(), 5);
    }

    #[test]
    fn multipv_lines_are_distinct_and_ordered() {
        let mut engine = Engine::new();
        engine.multipv = 3;
        let stop = AtomicBool::new(false);
        let mut infos = Vec::new();
        engine.search(
            &Position::startpos(),
            &[],
            &Limits::depth(5),
            &stop,
            &mut |info| infos.push(info.clone()),
        );
        let last: Vec<&SearchInfo> = infos.iter().filter(|i| i.depth == 5).collect();
        assert_eq!(last.len(), 3);
        let scores: Vec<i32> = last
            .iter()
            .map(|i| match i.score {
                Score::Centipawns(cp) => cp,
                Score::Mate(_) => panic!("no mates here"),
            })
            .collect();
        assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
        let firsts: std::collections::HashSet<Move> = last.iter().map(|i| i.pv[0]).collect();
        assert_eq!(firsts.len(), 3);
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
