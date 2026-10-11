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
//! ## Principal variation search (stage 11)
//!
//! With good move ordering the first move searched at a node is usually the best. **PVS**
//! bets on it: the first move gets the full window, and every later move is searched with a
//! **null window** `[alpha, alpha + 1]`, which only asks "is this move better than alpha?".
//! A null-window search cuts off far more often, so it's much cheaper. When the bet fails
//! (a later move does beat alpha), that move is searched again with the full window to get its
//! real score. With good ordering re-searches are rare and PVS wins overall.
//!
//! ## Aspiration windows (stage 11)
//!
//! Iterative deepening's scores rarely change much from one depth to the next. So instead of
//! searching each depth with the infinite window `[-∞, +∞]`, the root starts with a narrow
//! window around the previous score, `[score − 25, score + 25]`, which prunes much more. If
//! the true score falls outside (a **fail low** or **fail high**), the window is widened on
//! that side and the depth searched again, doubling the margin each time until it gives up and
//! uses the full window.
//!
//! ## Quiescence search
//!
//! Stopping the search at a fixed depth and evaluating there is dangerous: the last move
//! might be QxP with the pawn defended, and counting material right then says "a pawn up"
//! when the queen is about to be lost. This is the **horizon effect**. The fix is to keep
//! searching *captures only* past the nominal depth, until the position is quiet:
//!
//! - **Stand pat:** the side to move doesn't have to capture. Its static evaluation is a lower
//!   bound on its score, and if that's already `>= beta`, it can stop there.
//! - Otherwise it tries each capture (most valuable victim first, see [`crate::ordering`]) and
//!   keeps the best.
//! - **In check** there is no standing pat: every legal move is searched, so mates at the
//!   horizon are seen.
//!
//! Captures run out quickly, so the quiescence search always ends.
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
//! [Alpha-Beta](https://www.chessprogramming.org/Alpha-Beta),
//! [Quiescence Search](https://www.chessprogramming.org/Quiescence_Search) and
//! [Checkmate](https://www.chessprogramming.org/Checkmate).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use cb2026_chess::{Move, Position};

use crate::Score;
use crate::eval::{EvalParams, evaluate};
use crate::ordering::{MovePicker, OrderingTables, is_noisy};
use crate::tt::{Bound, TranspositionTable, score_from_tt, score_to_tt};

/// The score of being checkmated right now (higher than any evaluation).
pub const MATE: i32 = 32_000;
/// A score larger than any real score, for the initial search window.
pub const INFINITY: i32 = 32_001;
/// The deepest the search ever goes, in plies from the root.
pub const MAX_PLY: u32 = 128;
/// Search depth when the caller gives no limit at all (no depth, time, nodes or `infinite`).
pub const DEFAULT_DEPTH: u32 = 6;
/// How many nodes pass between checks of the stop flag, the clock and the node limit.
const CHECK_INTERVAL: u64 = 1024;

// ANCHOR: search_params
/// The search's tunable numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchParams {
    /// Aspiration windows start at the previous score ± this many centipawns.
    pub aspiration_delta: i32,
    /// Aspiration windows are used from this depth on (shallow scores jump around too much).
    pub aspiration_min_depth: u32,
    /// Once the margin grows past this, the full window is used.
    pub aspiration_max_delta: i32,
}

impl Default for SearchParams {
    fn default() -> SearchParams {
        SearchParams {
            aspiration_delta: 25,
            aspiration_min_depth: 5,
            aspiration_max_delta: 800,
        }
    }
}
// ANCHOR_END: search_params

/// Search techniques that can be switched off, to see what each one is worth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Techniques {
    /// Alpha-beta pruning. Off, the search is plain minimax: the same moves and scores, many
    /// more nodes.
    pub alpha_beta: bool,
    /// Quiescence search at the horizon. Off, positions at depth 0 are evaluated as they are,
    /// in the middle of exchanges.
    pub quiescence: bool,
    /// MVV-LVA move ordering: captures first, biggest victim first. Off, moves are searched
    /// in generation order.
    pub mvv_lva: bool,
    /// The transposition table: reuse results for positions already searched, and search
    /// their best move first. Off, the table is neither read nor written.
    pub tt: bool,
    /// Killer moves: quiet moves that caused a cutoff at the same ply are tried early.
    pub killers: bool,
    /// The countermove heuristic: the quiet move that last refuted the opponent's move is
    /// tried early.
    pub countermoves: bool,
    /// The history heuristic: quiet moves are ordered by how often they caused cutoffs.
    pub history: bool,
    /// Principal variation search: moves after the first get a null-window search first.
    pub pvs: bool,
    /// Aspiration windows: each iteration starts with a narrow window around the last score.
    pub aspiration: bool,
}

impl Default for Techniques {
    fn default() -> Techniques {
        Techniques {
            alpha_beta: true,
            quiescence: true,
            mvv_lva: true,
            tt: true,
            killers: true,
            countermoves: true,
            history: true,
            pvs: true,
            aspiration: true,
        }
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

/// The result of a root search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RootResult {
    pub best_move: Option<Move>,
    pub score: i32,
    pub pv: Vec<Move>,
    pub nodes: u64,
    pub seldepth: u32,
    pub depth: u32,
    /// Whether every root move was searched (false if the search was stopped part-way).
    pub complete: bool,
}

/// When a search must stop early.
#[derive(Clone, Copy)]
pub(crate) struct StopConditions<'a> {
    /// Set by another thread (the GUI's `stop`).
    pub stop: &'a AtomicBool,
    /// The hard time limit.
    pub deadline: Option<Instant>,
    /// The node limit.
    pub max_nodes: Option<u64>,
}

/// The state of one search.
pub(crate) struct Searcher<'a> {
    params: &'a EvalParams,
    search_params: SearchParams,
    tt: &'a mut TranspositionTable,
    ordering: &'a mut OrderingTables,
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
    /// The moves from the root to the current position (for the countermove heuristic).
    path: Vec<Move>,
}

impl<'a> Searcher<'a> {
    pub(crate) fn new(
        params: &'a EvalParams,
        search_params: SearchParams,
        tt: &'a mut TranspositionTable,
        ordering: &'a mut OrderingTables,
        techniques: Techniques,
        stop: StopConditions<'a>,
        history: &[u64],
    ) -> Searcher<'a> {
        Searcher {
            params,
            search_params,
            tt,
            ordering,
            techniques,
            stop: stop.stop,
            deadline: stop.deadline,
            max_nodes: stop.max_nodes,
            nodes: 0,
            seldepth: 0,
            aborted: false,
            keys: history.to_vec(),
            path: Vec::new(),
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
        if ply >= MAX_PLY {
            return evaluate(pos, self.params);
        }
        if depth == 0 {
            return if self.techniques.quiescence {
                self.quiescence(pos, ply, alpha, beta, pv)
            } else {
                evaluate(pos, self.params)
            };
        }

        // ANCHOR: tt_probe
        // Transposition table: a deep enough result may settle this position at once, and in
        // any case its best move is searched first.
        let mut tt_move = None;
        if self.techniques.tt
            && let Some(entry) = self.tt.probe(pos.hash())
        {
            tt_move = entry.best_move.filter(|mv| moves.contains(mv));
            if u32::from(entry.depth) >= depth {
                let score = score_from_tt(entry.score, ply);
                let usable = match entry.bound {
                    Bound::Exact => true,
                    Bound::Lower => score >= beta,
                    Bound::Upper => score <= alpha,
                };
                if usable {
                    pv.extend(tt_move);
                    return score;
                }
            }
        }
        // ANCHOR_END: tt_probe

        let previous = self.path.last().copied();
        let picker = MovePicker::new(
            pos,
            &moves,
            tt_move,
            ply,
            previous,
            self.ordering,
            &self.techniques,
        );
        let original_alpha = alpha;
        let mut best = -INFINITY;
        let mut best_move = None;
        let mut child_pv = Vec::new();
        // Quiet moves searched so far that didn't cause a cutoff (for the history penalty).
        let mut quiets_tried = Vec::new();
        self.keys.push(pos.hash());
        for (index, mv) in picker.enumerate() {
            let mut child = pos.clone();
            child.make_move(mv);
            self.path.push(mv);
            let score = self.search_move(&child, depth - 1, alpha, beta, index == 0, &mut child_pv);
            self.path.pop();
            if self.aborted {
                break;
            }
            if score > best {
                best = score;
                best_move = Some(mv);
                if score > alpha {
                    alpha = score;
                    pv.clear();
                    pv.push(mv);
                    pv.extend_from_slice(&child_pv);
                }
            }
            // Beta cutoff: the opponent won't allow this position, so stop looking.
            if self.techniques.alpha_beta && alpha >= beta {
                if !is_noisy(mv) {
                    self.ordering
                        .record_cutoff(pos, mv, &quiets_tried, ply, depth, previous);
                }
                break;
            }
            if !is_noisy(mv) {
                quiets_tried.push(mv);
            }
        }
        self.keys.pop();

        // ANCHOR: tt_store
        if self.techniques.tt && !self.aborted {
            let bound = if best >= beta {
                Bound::Lower
            } else if best > original_alpha {
                Bound::Exact
            } else {
                Bound::Upper
            };
            // After a fail low every move was refuted, so none of them is known to be best.
            let move_to_store = if bound == Bound::Upper {
                None
            } else {
                best_move
            };
            let score = score_to_tt(best, ply);
            self.tt
                .store(pos.hash(), move_to_store, score, depth, bound);
        }
        // ANCHOR_END: tt_store
        best
    }
    // ANCHOR_END: negamax

    // ANCHOR: pvs
    /// The score of the move that led to `child` (the last move on the path), from the
    /// mover's side: searched with the full window if it is the `first` move, otherwise (with
    /// PVS on) with a null window first, and again with the full window only if it beats
    /// `alpha`.
    fn search_move(
        &mut self,
        child: &Position,
        depth: u32,
        alpha: i32,
        beta: i32,
        first: bool,
        pv: &mut Vec<Move>,
    ) -> i32 {
        let ply = self.path.len() as u32;
        let pvs = self.techniques.pvs && self.techniques.alpha_beta;
        if first || !pvs {
            return -self.negamax(child, depth, ply, -beta, -alpha, pv);
        }
        let score = -self.negamax(child, depth, ply, -alpha - 1, -alpha, pv);
        if score > alpha && score < beta && !self.aborted {
            -self.negamax(child, depth, ply, -beta, -alpha, pv)
        } else {
            score
        }
    }
    // ANCHOR_END: pvs

    // ANCHOR: quiescence
    /// The quiescence score of `pos`, `ply` plies from the root, within `[alpha, beta]`:
    /// captures (and queen promotions) only, until the position is quiet. In check, every
    /// evasion is searched instead.
    fn quiescence(
        &mut self,
        pos: &Position,
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

        // In check every evasion counts; otherwise only captures and promotions are
        // generated. (So a stalemate reached by captures isn't noticed: a rare error that all
        // engines accept for the speed.)
        let in_check = pos.in_check();
        let moves = if in_check {
            let evasions = pos.legal_moves();
            if evasions.is_empty() {
                return -MATE + ply as i32;
            }
            evasions
        } else {
            pos.noisy_moves()
        };
        if ply >= MAX_PLY {
            return evaluate(pos, self.params);
        }

        let mut best = -INFINITY;
        if !in_check {
            // Stand pat: not capturing is an option, so the static score is a lower bound.
            best = evaluate(pos, self.params);
            if self.techniques.alpha_beta && best >= beta {
                return best;
            }
            alpha = alpha.max(best);
        }

        // Under-promotions are generated but almost never matter: skip them.
        let moves: Vec<Move> = moves
            .iter()
            .copied()
            .filter(|&mv| in_check || is_noisy(mv))
            .collect();
        let picker = MovePicker::new(
            pos,
            &moves,
            None,
            ply,
            None,
            self.ordering,
            &self.techniques,
        );
        let mut child_pv = Vec::new();
        for mv in picker {
            let mut child = pos.clone();
            child.make_move(mv);
            let score = -self.quiescence(&child, ply + 1, -beta, -alpha, &mut child_pv);
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
            if self.techniques.alpha_beta && alpha >= beta {
                break;
            }
        }
        best
    }
    // ANCHOR_END: quiescence

    /// Searches the root position to `depth` within `[alpha, beta]`, trying `root_moves` in
    /// the order given.
    ///
    /// The score is fail-soft: at most `alpha` if every move failed low, at least `beta` if a
    /// move failed high (the search then stops, since the caller will widen the window).
    /// If the search is stopped early, the best fully searched move is kept and `complete` is
    /// false; if not even one move was finished, `best_move` is the first move and `score`
    /// is `-INFINITY`.
    pub(crate) fn search_root(
        &mut self,
        pos: &Position,
        root_moves: &[Move],
        depth: u32,
        mut alpha: i32,
        beta: i32,
    ) -> RootResult {
        let mut result = RootResult {
            best_move: root_moves.first().copied(),
            score: -INFINITY,
            depth,
            ..RootResult::default()
        };
        let mut child_pv = Vec::new();
        self.keys.push(pos.hash());
        for (index, &mv) in root_moves.iter().enumerate() {
            let mut child = pos.clone();
            child.make_move(mv);
            self.path.push(mv);
            let score = self.search_move(
                &child,
                depth.saturating_sub(1),
                alpha,
                beta,
                index == 0,
                &mut child_pv,
            );
            self.path.pop();
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
                    if score >= beta {
                        break;
                    }
                }
            }
        }
        self.keys.pop();
        result.nodes = self.nodes;
        result.seldepth = self.seldepth;
        result.complete = !self.aborted;
        result
    }

    // ANCHOR: aspiration
    /// Searches the root to `depth`, using an aspiration window around `previous` (the last
    /// depth's score) when there is one. Fail-high moves are moved to the front of
    /// `root_moves` for the re-search.
    pub(crate) fn search_depth(
        &mut self,
        pos: &Position,
        root_moves: &mut Vec<Move>,
        depth: u32,
        previous: Option<i32>,
    ) -> RootResult {
        let p = self.search_params;
        let center = previous.filter(|score| {
            self.techniques.aspiration
                && self.techniques.alpha_beta
                && depth >= p.aspiration_min_depth
                && score.abs() < MATE - MAX_PLY as i32
        });
        let Some(center) = center else {
            return self.search_root(pos, root_moves, depth, -INFINITY, INFINITY);
        };
        let mut delta = p.aspiration_delta;
        let (mut alpha, mut beta) = (center - delta, center + delta);
        let mut fail_high: Option<RootResult> = None;
        loop {
            let result = self.search_root(pos, root_moves, depth, alpha, beta);
            if !result.complete {
                // Stopped during a re-search before any move finished: the move that failed
                // high is still the best information there is.
                return match fail_high {
                    Some(high) if result.score == -INFINITY => RootResult {
                        complete: false,
                        nodes: result.nodes,
                        seldepth: result.seldepth,
                        ..high
                    },
                    _ => result,
                };
            }
            if result.score <= alpha {
                alpha = (result.score - delta).max(-INFINITY);
            } else if result.score >= beta {
                beta = (result.score + delta).min(INFINITY);
                if let Some(mv) = result.best_move {
                    root_moves.retain(|&m| m != mv);
                    root_moves.insert(0, mv);
                }
                fail_high = Some(result);
            } else {
                return result;
            }
            delta *= 2;
            if delta > p.aspiration_max_delta {
                (alpha, beta) = (-INFINITY, INFINITY);
            }
        }
    }
    // ANCHOR_END: aspiration

    /// Whether the search has been stopped (stop flag, hard time limit or node limit).
    pub(crate) fn aborted(&self) -> bool {
        self.aborted
    }

    /// How full the transposition table is, in permille.
    pub(crate) fn hashfull(&self) -> u32 {
        self.tt.hashfull()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(fen: &str, depth: u32, techniques: Techniques) -> RootResult {
        search_with_window(fen, depth, techniques, None)
    }

    /// Searches to `depth`, with an aspiration window around `previous` if given.
    fn search_with_window(
        fen: &str,
        depth: u32,
        techniques: Techniques,
        previous: Option<i32>,
    ) -> RootResult {
        let pos = Position::from_fen(fen).unwrap();
        // Material only, so the expected scores can be worked out by hand.
        let params = EvalParams::material_only();
        let stop = AtomicBool::new(false);
        let mut tt = TranspositionTable::new(1);
        let mut ordering = OrderingTables::default();
        let mut searcher = Searcher::new(
            &params,
            SearchParams::default(),
            &mut tt,
            &mut ordering,
            techniques,
            StopConditions {
                stop: &stop,
                deadline: None,
                max_nodes: None,
            },
            &[],
        );
        let mut moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        searcher.search_depth(&pos, &mut moves, depth, previous)
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
        // Without quiescence (a minimax quiescence search would take minutes) or the
        // transposition table (its bounds and cutoffs come from alpha-beta).
        let plain = Techniques {
            quiescence: false,
            tt: false,
            ..Techniques::default()
        };
        let with = search(fen, 3, plain);
        let without = search(
            fen,
            3,
            Techniques {
                alpha_beta: false,
                ..plain
            },
        );
        assert_eq!(with.score, without.score);
        // With only MVV-LVA ordering (stage 10 does much better) alpha-beta is far from its
        // best case, but it still cuts the tree down a lot.
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
    fn quiescence_sees_past_the_horizon() {
        // Qxd5 wins a pawn at depth 1, but exd5 wins the queen back.
        let fen = "4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1";
        let blind = search(
            fen,
            1,
            Techniques {
                quiescence: false,
                ..Techniques::default()
            },
        );
        // A queen against two pawns is +700; the blind search thinks it's +800 after Qxd5.
        assert_eq!(blind.best_move.unwrap().to_string(), "d1d5");
        assert_eq!(blind.score, 800);
        let result = search(fen, 1, Techniques::default());
        assert_ne!(result.best_move.unwrap().to_string(), "d1d5");
        assert_eq!(result.score, 700);
        assert!(result.seldepth > 1);
    }

    #[test]
    fn quiescence_finds_mate_at_the_horizon() {
        // At depth 1 White plays Re8+ and the quiescence search, in check, finds no evasion.
        let result = search(
            "6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1",
            1,
            Techniques::default(),
        );
        assert_eq!(to_score(result.score), Score::Mate(1));
    }

    #[test]
    fn mvv_lva_gives_the_same_score_with_fewer_nodes() {
        // A quiet opening position: in a tactical one like Kiwipete, the unordered quiescence
        // search takes millions of nodes. The quiet-move heuristics are off, to measure
        // MVV-LVA alone.
        let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";
        let plain = Techniques {
            killers: false,
            countermoves: false,
            history: false,
            ..Techniques::default()
        };
        let ordered = search(fen, 4, plain);
        let unordered = search(
            fen,
            4,
            Techniques {
                mvv_lva: false,
                ..plain
            },
        );
        assert_eq!(ordered.score, unordered.score);
        assert!(
            ordered.nodes * 2 < unordered.nodes,
            "{} vs {}",
            ordered.nodes,
            unordered.nodes
        );
    }

    #[test]
    fn quiet_move_ordering_gives_the_same_scores_with_fewer_nodes() {
        // Over a few positions: on any single one a heuristic can be unlucky.
        let fens = [
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
            "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1",
            "r2q1rk1/pp2bppp/2n1pn2/3p4/3P4/2NBPN2/PP3PPP/R2Q1RK1 w - - 0 10",
        ];
        let off = Techniques {
            killers: false,
            countermoves: false,
            history: false,
            ..Techniques::default()
        };
        let (mut with_nodes, mut without_nodes) = (0, 0);
        for fen in fens {
            let with = search(fen, 4, Techniques::default());
            let without = search(fen, 4, off);
            assert_eq!(with.score, without.score, "{fen}");
            with_nodes += with.nodes;
            without_nodes += without.nodes;
        }
        assert!(
            with_nodes < without_nodes,
            "{with_nodes} vs {without_nodes}"
        );
    }

    #[test]
    fn pvs_gives_the_same_score_with_fewer_nodes() {
        let fen = "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1";
        let with = search(fen, 5, Techniques::default());
        let without = search(
            fen,
            5,
            Techniques {
                pvs: false,
                ..Techniques::default()
            },
        );
        assert_eq!(with.score, without.score);
        assert!(
            with.nodes < without.nodes,
            "{} vs {}",
            with.nodes,
            without.nodes
        );
    }

    #[test]
    fn a_failed_aspiration_window_is_widened_until_the_score_fits() {
        // Without the table, so every search sees the same tree.
        let techniques = Techniques {
            tt: false,
            ..Techniques::default()
        };
        let fen = "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1";
        let full = search_with_window(fen, 5, techniques, None);
        // A guess far too low (fails high), then one far too high (fails low).
        for guess in [full.score - 300, full.score + 300] {
            let windowed = search_with_window(fen, 5, techniques, Some(guess));
            assert_eq!(windowed.score, full.score, "guess {guess}");
            assert_eq!(windowed.best_move, full.best_move, "guess {guess}");
        }
    }

    #[test]
    fn mate_scores_convert_to_moves() {
        assert_eq!(to_score(MATE - 1), Score::Mate(1));
        assert_eq!(to_score(MATE - 3), Score::Mate(2));
        assert_eq!(to_score(-MATE + 2), Score::Mate(-1));
        assert_eq!(to_score(57), Score::Centipawns(57));
    }
}
