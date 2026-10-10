//! Search: looking ahead to find the best move.
//!
//! ## Minimax and negamax
//!
//! To choose a move, imagine every move, every reply, every reply to that, and so on to some
//! depth; evaluate the positions at the end; then work backwards assuming each side picks
//! what's best for it. That's **minimax**. Chess scores are symmetric (good for me = bad for
//! you), so it can be written as one function, **negamax**: a position's score is the best,
//! over all moves, of *minus* the score of the position after the move.
//!
//! ## Alpha-beta
//!
//! Most of that tree doesn't matter. Searching with a window `[alpha, beta]` ("I can already
//! get alpha somewhere else; my opponent can already hold me to beta"), as soon as one move
//! scores at least `beta`, the opponent would never allow this position, and the remaining
//! moves can be skipped: a **beta cutoff**. The result is exactly the same as minimax, but
//! with good move ordering the number of positions searched drops from about `b^d` to about
//! `b^(d/2)`: twice the depth for the same effort.
//!
//! ## Scores for mate and draws
//!
//! - **Checkmate** is scored `-MATE + ply` for the side that is mated, so a quicker mate scores
//!   higher (for the winner) and the engine plays the fastest one.
//! - **Stalemate**, the **fifty-move rule**, **insufficient material** and **repetition** score
//!   0. Inside the search a single repetition counts as a draw: if a position can be repeated
//!   once, it can be repeated again.
//!
//! See the Chess Programming Wiki, [Negamax](https://www.chessprogramming.org/Negamax),
//! [Alpha-Beta](https://www.chessprogramming.org/Alpha-Beta) and
//! [Checkmate](https://www.chessprogramming.org/Checkmate).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cb2026_chess::{Color, Move, Position};

use crate::eval::{EvalParams, evaluate};
use crate::{Limits, Score};

/// The score of being checkmated right now (higher than any evaluation).
pub const MATE: i32 = 32_000;
/// A score larger than any real score, for the initial search window.
pub const INFINITY: i32 = 32_001;
/// The deepest the search ever goes, in plies from the root.
pub const MAX_PLY: u32 = 128;
/// Search depth when the caller gives no depth (stage 6 replaces this with iterative
/// deepening and time management).
pub const DEFAULT_DEPTH: u32 = 4;
/// How many nodes pass between checks of the stop flag, the clock and the node limit.
const CHECK_INTERVAL: u64 = 1024;
/// When playing on a clock, spend about this fraction of the remaining time on one move.
const TIME_FRACTION: u32 = 30;

/// Search techniques that can be switched off, to see what each one is worth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Techniques {
    /// Alpha-beta pruning. Off, the search is plain minimax: the same moves and scores, many
    /// more nodes.
    pub alpha_beta: bool,
}

impl Default for Techniques {
    fn default() -> Techniques {
        Techniques { alpha_beta: true }
    }
}

/// Converts an internal score to a [`Score`], turning mate scores into "mate in N moves".
pub fn to_score(value: i32) -> Score {
    if value.abs() >= MATE - MAX_PLY as i32 {
        let plies = MATE - value.abs();
        let moves = (plies + 1) / 2;
        Score::Mate(if value > 0 { moves } else { -moves })
    } else {
        Score::Centipawns(value)
    }
}

/// The time to stop searching, if the limits set one.
fn deadline(start: Instant, limits: &Limits, side: Color) -> Option<Instant> {
    if let Some(movetime) = limits.movetime {
        return Some(start + movetime);
    }
    let remaining = limits.time[side.index()]?;
    let increment = limits.increment[side.index()].unwrap_or(Duration::ZERO);
    let moves_left = limits
        .moves_to_go
        .unwrap_or(TIME_FRACTION)
        .clamp(1, TIME_FRACTION);
    // A share of the remaining time plus most of the increment, never more than half the clock.
    let budget = (remaining / moves_left + increment * 3 / 4).min(remaining / 2);
    Some(start + budget)
}

/// The result of a root search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RootResult {
    pub best_move: Option<Move>,
    pub score: i32,
    pub pv: Vec<Move>,
    pub nodes: u64,
    pub seldepth: u32,
    pub depth: u32,
}

/// The state of one search.
pub(crate) struct Searcher<'a> {
    params: &'a EvalParams,
    techniques: Techniques,
    stop: &'a AtomicBool,
    deadline: Option<Instant>,
    max_nodes: Option<u64>,
    nodes: u64,
    seldepth: u32,
    aborted: bool,
    /// Hash keys of the positions before the current one: the game so far, then the search
    /// path. Used to detect repetitions.
    keys: Vec<u64>,
}

impl<'a> Searcher<'a> {
    pub(crate) fn new(
        params: &'a EvalParams,
        techniques: Techniques,
        stop: &'a AtomicBool,
        limits: &Limits,
        root: &Position,
        history: &[u64],
        start: Instant,
    ) -> Searcher<'a> {
        Searcher {
            params,
            techniques,
            stop,
            deadline: deadline(start, limits, root.side_to_move()),
            max_nodes: limits.nodes,
            nodes: 0,
            seldepth: 0,
            aborted: false,
            keys: history.to_vec(),
        }
    }

    /// Whether the search must end now (stop flag, time or node limit). Checked every
    /// [`CHECK_INTERVAL`] nodes, since reading the clock isn't free.
    fn should_stop(&mut self) -> bool {
        if !self.aborted && self.nodes.is_multiple_of(CHECK_INTERVAL) {
            self.aborted = self.stop.load(Ordering::Relaxed)
                || self.deadline.is_some_and(|d| Instant::now() >= d)
                || self.max_nodes.is_some_and(|n| self.nodes >= n);
        }
        self.aborted
    }

    /// Whether `pos` is drawn by the fifty-move rule, insufficient material or repetition.
    fn is_draw(&self, pos: &Position) -> bool {
        if pos.halfmove_clock() >= 100 || pos.is_insufficient_material() {
            return true;
        }
        // A repetition can only reach back to the last capture or pawn move, and only to
        // positions with the same side to move (every second entry).
        let reach = (pos.halfmove_clock() as usize).min(self.keys.len());
        self.keys[self.keys.len() - reach..]
            .iter()
            .rev()
            .skip(1)
            .step_by(2)
            .any(|&key| key == pos.hash())
    }

    // ANCHOR: negamax
    /// The negamax score of `pos` searched `depth` plies deep, `ply` plies from the root,
    /// within the window `[alpha, beta]`. The best line found is written to `pv`.
    fn negamax(
        &mut self,
        pos: &Position,
        depth: u32,
        ply: u32,
        mut alpha: i32,
        beta: i32,
        pv: &mut Vec<Move>,
    ) -> i32 {
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);
        pv.clear();
        if self.should_stop() {
            return 0;
        }

        let moves = pos.legal_moves();
        if moves.is_empty() {
            // Checkmate (worse the sooner it happens) or stalemate.
            return if pos.in_check() {
                -MATE + ply as i32
            } else {
                0
            };
        }
        if self.is_draw(pos) {
            return 0;
        }
        if depth == 0 || ply >= MAX_PLY {
            return evaluate(pos, self.params);
        }

        let mut best = -INFINITY;
        let mut child_pv = Vec::new();
        self.keys.push(pos.hash());
        for &mv in moves.iter() {
            let mut child = pos.clone();
            child.make_move(mv);
            let score = -self.negamax(&child, depth - 1, ply + 1, -beta, -alpha, &mut child_pv);
            if self.aborted {
                break;
            }
            if score > best {
                best = score;
                if score > alpha {
                    alpha = score;
                    pv.clear();
                    pv.push(mv);
                    pv.extend_from_slice(&child_pv);
                }
            }
            // Beta cutoff: the opponent won't allow this position, so stop looking.
            if self.techniques.alpha_beta && alpha >= beta {
                break;
            }
        }
        self.keys.pop();
        best
    }
    // ANCHOR_END: negamax

    /// Searches the root position to `depth`, considering only `root_moves`.
    ///
    /// If the search is stopped early, the best move found so far is kept (or, if not even one
    /// move was searched, the first legal move, so there's always something to play).
    pub(crate) fn search_root(
        &mut self,
        pos: &Position,
        root_moves: &[Move],
        depth: u32,
    ) -> RootResult {
        let mut result = RootResult {
            best_move: root_moves.first().copied(),
            score: -INFINITY,
            depth,
            ..RootResult::default()
        };
        let mut alpha = -INFINITY;
        let mut child_pv = Vec::new();
        self.keys.push(pos.hash());
        for &mv in root_moves {
            let mut child = pos.clone();
            child.make_move(mv);
            let score = -self.negamax(
                &child,
                depth.saturating_sub(1),
                1,
                -INFINITY,
                -alpha,
                &mut child_pv,
            );
            if self.aborted {
                break;
            }
            if score > result.score {
                result.score = score;
                result.best_move = Some(mv);
                result.pv = std::iter::once(mv)
                    .chain(child_pv.iter().copied())
                    .collect();
                if self.techniques.alpha_beta {
                    alpha = alpha.max(score);
                }
            }
        }
        self.keys.pop();
        result.nodes = self.nodes;
        result.seldepth = self.seldepth;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(fen: &str, depth: u32, techniques: Techniques) -> RootResult {
        let pos = Position::from_fen(fen).unwrap();
        let params = EvalParams::default();
        let stop = AtomicBool::new(false);
        let limits = Limits::depth(depth);
        let mut searcher = Searcher::new(
            &params,
            techniques,
            &stop,
            &limits,
            &pos,
            &[],
            Instant::now(),
        );
        let moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        searcher.search_root(&pos, &moves, depth)
    }

    #[test]
    fn finds_mate_in_one() {
        // Back-rank mate: Re8#.
        let result = search(
            "6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1",
            2,
            Techniques::default(),
        );
        assert_eq!(result.best_move.unwrap().to_string(), "e1e8");
        assert_eq!(to_score(result.score), Score::Mate(1));
    }

    #[test]
    fn finds_mate_in_two() {
        // Doubled rooks on the back rank: 1. Rd8+ Rxd8 2. Rxd8#.
        let result = search(
            "2r3k1/5ppp/8/8/8/8/3R1PPP/3R2K1 w - - 0 1",
            4,
            Techniques::default(),
        );
        assert_eq!(to_score(result.score), Score::Mate(2));
    }

    #[test]
    fn wins_a_hanging_queen() {
        let result = search(
            "4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1",
            2,
            Techniques::default(),
        );
        assert_eq!(result.best_move.unwrap().to_string(), "e4d5");
    }

    #[test]
    fn alpha_beta_gives_the_minimax_score_with_fewer_nodes() {
        let fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        let with = search(fen, 3, Techniques::default());
        let without = search(fen, 3, Techniques { alpha_beta: false });
        assert_eq!(with.score, without.score);
        // Without move ordering (stage 10) alpha-beta is far from its best case, but it still
        // cuts the tree down a lot.
        assert!(
            with.nodes * 2 < without.nodes,
            "{} vs {}",
            with.nodes,
            without.nodes
        );
    }

    #[test]
    fn stalemate_and_draws_score_zero() {
        // Black to move is stalemated.
        let result = search("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", 3, Techniques::default());
        assert_eq!(result.best_move, None);
        // Up a rook but at move 100 of the fifty-move rule: every move draws.
        let result = search("4k3/8/8/8/8/8/8/R3K3 w - - 99 80", 2, Techniques::default());
        assert_eq!(result.score, 0);
    }

    #[test]
    fn mate_scores_convert_to_moves() {
        assert_eq!(to_score(MATE - 1), Score::Mate(1));
        assert_eq!(to_score(MATE - 3), Score::Mate(2));
        assert_eq!(to_score(-MATE + 2), Score::Mate(-1));
        assert_eq!(to_score(57), Score::Centipawns(57));
    }

    #[test]
    fn clock_budget() {
        let start = Instant::now();
        let limits = Limits {
            time: [Some(Duration::from_secs(60)), None],
            increment: [Some(Duration::from_secs(1)), None],
            ..Limits::default()
        };
        let budget = deadline(start, &limits, Color::White).unwrap() - start;
        assert_eq!(budget, Duration::from_millis(2000 + 750));
        assert_eq!(
            deadline(start, &limits, Color::Black),
            None,
            "no clock for Black"
        );
    }
}
