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
//! ## Null-move pruning (stage 12)
//!
//! In almost every chess position, moving is better than passing. So if a side could *pass*
//! and still be doing so well that the opponent can't bring the score below beta, then with a
//! real move it would surely do even better: the node can be cut off without searching any
//! move. The pass (a **null move**) is searched with a reduced depth and a null window around
//! beta, which makes the test cheap.
//!
//! The assumption fails in **zugzwang**, where any move makes things worse (common in pawn
//! endgames), so null moves are not tried without pieces other than pawns. They are also not
//! tried in check (passing would be illegal), twice in a row, or on the principal variation.
//!
//! ## Late move reductions and late move pruning (stage 13)
//!
//! With good ordering, a cutoff almost always comes from one of the first few moves. Moves
//! late in the list are rarely best, so **late move reductions** (LMR) search them less
//! deeply: the reduction grows with the depth and with the move's position in the list
//! (`0.75 + ln(depth) × ln(move number) / 2.25` plies). If a reduced move surprises by
//! beating alpha, it is searched again at full depth. Captures, checks, moves while in check
//! and the first moves are never reduced.
//!
//! **Late move pruning** (LMP) goes further near the leaves: at depth 3 or less, outside the
//! principal variation, once enough quiet moves have been tried (`3 + depth²`), the remaining
//! quiet moves are not searched at all.
//!
//! ## Futility, razoring and SEE pruning (stage 14)
//!
//! More selective pruning near the leaves, all based on how far the static evaluation is from
//! the window, and none of it on the principal variation or in check:
//!
//! - **Reverse futility pruning** (static null move): if the static evaluation beats beta by
//!   a margin that grows with depth, assume the search would too and return at once.
//! - **Razoring**: if the static evaluation is far below alpha at depth 1 or 2, ask the
//!   quiescence search; if even the captures can't lift it to alpha, give up on the node.
//! - **Futility pruning**: at low depth, a quiet move can't raise a static evaluation that is
//!   hopelessly below alpha, so quiet moves are skipped.
//! - **SEE pruning**: moves that lose material in the static exchange (see [`crate::see`])
//!   are skipped at low depth, and bad captures are skipped in the quiescence search.
//! - **Delta pruning** in the quiescence search: a capture that can't bring the score near
//!   alpha even if it wins the captured piece outright is skipped.
//!
//! ## Check extensions
//!
//! Reductions search unpromising moves less deeply; an **extension** searches a forcing move
//! more deeply. A move that gives check is searched one ply deeper than the rest: a check
//! leaves the opponent few replies, so the extra ply is cheap, and checks are where the
//! horizon hides mates and lost pieces. Extensions are limited to the first half of
//! [`MAX_PLY`], so a long series of checks can't grow the search without bound.
//!
//! **Off by default.** Extending every check made the search about 24% bigger, and an SPRT
//! against the same engine without it found no gain: −8 ± 18 Elo after 840 games at 8+0.08.
//! Late move reductions already spare checking moves, and the quiescence search follows the
//! captures, so most of what the extension would find is found anyway. Strong engines extend
//! more selectively (for example only checks that don't lose material, or only "singular"
//! moves). The switch stays, so the experiment can be repeated.
//!
//! ## Internal iterative reductions
//!
//! The transposition table's best move is usually the best move, and searching it first is
//! what makes alpha-beta fast. A node at some depth with no hash move has never been searched
//! (or always failed low), so its move ordering will be poor; and nodes like that are rarely on
//! the principal variation. **Internal iterative reduction** (IIR) searches such nodes one ply
//! shallower, from depth 4 on. If the node matters, the next iteration finds it again, now with
//! a hash move, at full depth. (The older **internal iterative deepening** did a shallow search
//! first just to find a move to order first; reducing turned out to be cheaper and as good.)
//!
//! ## Improving
//!
//! A side whose static evaluation is better than two plies ago, the last time it was to
//! move, is **improving**: its moves are working, and its evaluation will probably keep rising.
//! Reverse futility pruning, which bets that a node far above beta stays there, can then bet
//! with a smaller margin: half a ply's worth smaller. (Stockfish uses the same idea in many more
//! places.) Late move pruning can use it too, trying more quiet moves before giving up in an
//! improving position, where they're more often good; that is the parameter `lmp_improving`,
//! 1 (no change) by default.
//!
//! **The margin never reaches zero.** The first version shrank the reverse futility margin by a
//! whole ply, as Stockfish 11 (GPL-3.0) did. At depth 1 that left no margin at all: the node was
//! cut off whenever its static evaluation reached beta, and cb2026's evaluation knows nothing
//! about pieces under attack. That version made the search almost a third smaller and the
//! engine weaker: −32 ± 15 Elo after 1,142 games at 8+0.08 (cjus/chessengine#60).
//!
//! **Late move pruning is left alone.** The second version, half a ply off the margin, also
//! measured weaker: −18 ± 12 Elo after 1,691 games (cjus/chessengine#63). Both versions also
//! doubled late move pruning's threshold when improving (`lmp_improving` = 2), so the cost may
//! have been there; the third version changed reverse futility pruning only. Both tests also ran
//! with a bug, found afterwards and fixed in #63: the root didn't store its evaluation, so every
//! node two plies down counted as improving.
//!
//! **Off by default.** The third version measured worse still: −33 ± 15 Elo after 1,036 games
//! (cjus/chessengine#65). So the cost is in reverse futility pruning: a smaller margin when
//! improving loses here, and the doubled late move pruning of the second version seems to have
//! made up some of it. The switch and both parameters stay, so the parts can be tested again.
//!
//! ## History in late move reductions
//!
//! LMR reduces a quiet move by how late it comes in the ordering. But the order of the quiet
//! moves comes from the history table, which is coarse, and a move with a high history score
//! that still came late has caused cutoffs elsewhere: it deserves a closer look. So the
//! reduction shrinks by a ply for every `lmr_history_divisor` (256) points of history score, by
//! at most `lmr_history_max` (2) plies, and grows the same way for a negative score.
//!
//! The divisor comes from measuring. Over a 40-move self-play game, about three quarters of the
//! moves LMR reduced had history scores within ±64; about one in ten had 256 or more; almost
//! none had less than −256. The scores lean positive because penalties go only to the quiet
//! moves tried before a quiet move's cutoff: at a node where no quiet move cuts off, none of
//! them is penalised. So in practice this version reduces good moves less and hardly ever
//! reduces any move more. Stockfish (GPL-3.0) does the same with its history and continuation
//! history added up; the code here is cb2026's own.
//!
//! **The first version had no verdict.** An SPRT against `main` without it (with continuation
//! history) found no clear difference: +2.1 ± 5.0 Elo after 8,910 games at 8+0.08, stopped
//! there (cjus/chessengine#70). It changed about one reduction in ten, by a ply or two.
//!
//! **The second version** uses the score that orders the quiet moves (history plus continuation
//! history, [`OrderingTables::quiet_score`]) and a divisor of 128. Over a 40-move self-play
//! game, 86% of reduced moves had that score within ±128; about 9% had 128 or more and 5% had
//! −128 or less. So it changes about one reduction in seven, and for the first time reduces a
//! move with a bad score more as well as a good one less. It passed: +22.2 ± 11.4 Elo after
//! 1,661 games at 8+0.08 (cjus/chessengine#74).
//!
//! ## Singular extensions and multi-cut
//!
//! A hash move is **singular** if it is much better than every other move: then it is probably
//! forced (a recapture, the one defence to a threat), and a mistake in its line costs the most,
//! so it is searched a ply deeper. To find out, the node is searched again first, at half the
//! depth and without the hash move, with a null window at `singular_beta`, the hash move's
//! score less `singular_margin` (3) centipawns per ply of depth. If no other move reaches it,
//! the hash move is singular. This needs a hash entry that is a lower bound or exact and at
//! most `singular_tt_depth_margin` (3) plies shallower than the node, from depth
//! `singular_min_depth` (4) on, and stops at twice the root depth, so that a chain of
//! extensions can't grow without limit.
//!
//! That search can also answer a second question. If another move beats `singular_beta` and
//! `singular_beta` is at least beta, two moves beat beta, so the node is cut off: **multi-cut**.
//!
//! And a third. If the hash move isn't singular but its hash score beats beta, the node will
//! probably cut off whichever of the good moves is searched first, and the hash move's exact
//! line matters less: it is searched `negative_extension_plies` (1) shallower instead, a
//! **negative extension**.
//!
//! And a fourth. The search's score is the best any other move managed, so it says by how much
//! the hash move is singular. If no other move comes within `double_extension_margin` (20)
//! centipawns of `singular_beta` either, the hash move is close to the only move, and at a node
//! off the principal variation it is searched two plies deeper instead of one: a **double
//! extension**. A line can have at most `double_extension_limit` (6) of them, on top of the
//! limit of twice the root depth that every extension has.
//!
//! The search without the hash move is a search of a different question, so it neither reads
//! nor writes the transposition table, tries no null move and does no IIR. If the hash move is
//! the only legal move, it fails low: nothing else comes close.
//!
//! All four are as in Stockfish (GPL-3.0); the code is cb2026's own. Singular extensions go
//! back to Deep Thought (Anantharaman, Campbell and Hsu, 1988).
//!
//! They make the bench tree about half as big again, and the engine stronger: +29.5 ± 13.3 Elo
//! after 1,357 games at 8+0.08 (cjus/chessengine#71). Negative extensions came later: they take
//! 2.3% off the bench and gained +4.9 ± 4.0 Elo after 14,211 games (cjus/chessengine#77).
//! Starting the test from depth 6 instead of 8 gained +5.0 ± 4.1 Elo after 12,282 games
//! (cjus/chessengine#78), and from depth 4 instead of 6 another +6.1 ± 4.8 after 9,057
//! (cjus/chessengine#80). A `singular_margin` of 3 instead of 2 gained +5.2 ± 4.2 after 11,400
//! (cjus/chessengine#82). Double extensions, which make the bench 14% bigger, had no verdict:
//! +3.6 ± 4.9 Elo after 8,670 games, stopped there (cjus/chessengine#96). They are off by
//! default.
//!
//! ## Correction history
//!
//! The pruning decisions above all trust the static evaluation, and a hand-crafted evaluation
//! is wrong in the same way in the same pawn structures. **Correction history** learns how far
//! off it was from the nodes' searched scores, by side to move and pawn structure, and corrects
//! every static evaluation in the main and quiescence searches by that much (see
//! [`crate::correction`]). It takes 3% off the bench and gained +28.0 ± 13.0 Elo after 1,308
//! games at 8+0.08 (cjus/chessengine#112).
//!
//! ## ProbCut
//!
//! Null-move pruning asks whether passing keeps the score at or above beta. **ProbCut** asks
//! whether a good capture keeps it well above: if some capture beats `beta + probcut_margin`
//! (100 centipawns) in a search `probcut_reduction` (4) plies shallower, the full-depth search
//! would almost surely beat beta, so the node is cut off. A shallow search's score and a deep
//! one's are closely related, and the margin covers most of the difference.
//!
//! The test is cheap. Only captures and queen promotions are tried, only those that win by
//! static exchange at least what the static evaluation is short of the margin, and each gets a
//! quiescence search first, with the deeper search only if that reaches the margin too. It
//! applies off the principal variation, out of check and from depth `probcut_min_depth` (5).
//! It is skipped when beta is near a mate score, and when the hash table already holds a
//! result about as deep as ProbCut's own search that falls short of the margin. A cutoff is
//! stored in the table as a lower bound at the depth ProbCut searched to, plus one.
//!
//! Michael Buro invented ProbCut for Othello (his thesis, 1994; "ProbCut: An Effective
//! Selective Extension of the Alpha-Beta Algorithm", ICCA Journal 18(2), 1995). His version
//! sets the margin from the statistics of shallow and deep scores. Chess engines found little
//! in it until Gary Linscott's version for Stockfish (GPL-3.0, 2013), which tries captures
//! against a fixed margin. This is that kind, and the code is cb2026's own. See the Chess
//! Programming Wiki, [ProbCut](https://www.chessprogramming.org/ProbCut).
//!
//! **Off by default.** It took 9.6% off the bench and measured weaker: −10.2 ± 9.1 Elo after
//! 2,593 games at 8+0.08 (cjus/chessengine#117). The switch and the parameters stay, so other
//! settings can be tested.
//!
//! ## Tablebase probes (stage 18)
//!
//! With few pieces left, the search can ask the endgame tablebases for the exact result
//! instead of searching on (see [`crate::tablebase`]).
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

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use cb2026_chess::{Move, PieceType, Position};

use crate::Score;
use crate::correction::CorrectionHistory;
use crate::eval::{EvalParams, evaluate};
use crate::ordering::{
    CONTINUATION_PLIES, MovePicker, OrderingTables, PieceTo, Previous, is_noisy,
};
use crate::see::{SEE_VALUES, see};
use crate::stats::{Note, SearchStats, SearchTree};
use crate::tablebase::{self, Tablebase, TablebaseParams};
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
    /// Null moves are tried with at least this much depth left.
    pub null_min_depth: u32,
    /// The null move's search is reduced by this many plies...
    pub null_reduction: u32,
    /// ...plus one more per this many plies of depth.
    pub null_depth_divisor: u32,
    /// Late move reductions start at this depth...
    pub lmr_min_depth: u32,
    /// ...for moves after this many have been searched.
    pub lmr_min_moves: u32,
    /// The reduction's constant part, in hundredths of a ply.
    pub lmr_base: u32,
    /// The divisor of `ln(depth) × ln(move number)`, in hundredths.
    pub lmr_divisor: u32,
    /// A quiet move's reduction shrinks by a ply for each this much history score it has
    /// (and grows by a ply for each this much below zero)...
    pub lmr_history_divisor: i32,
    /// ...by at most this many plies either way.
    pub lmr_history_max: i32,
    /// Late move pruning applies up to this depth...
    pub lmp_max_depth: u32,
    /// ...once `lmp_base + depth²` quiet moves have been tried...
    pub lmp_base: u32,
    /// ...times this when the side to move is improving (1: no change).
    pub lmp_improving: u32,
    /// Reverse futility pruning applies up to this depth...
    pub rfp_max_depth: u32,
    /// ...when the static evaluation beats beta by this much per ply of depth...
    pub rfp_margin: i32,
    /// ...less this much when the side to move is improving. Keep it below `rfp_margin`, so
    /// the margin never reaches zero.
    pub rfp_improving: i32,
    /// Razoring applies up to this depth...
    pub razor_max_depth: u32,
    /// ...when the static evaluation is below alpha by this much per ply of depth.
    pub razor_margin: i32,
    /// Futility pruning applies up to this depth...
    pub futility_max_depth: u32,
    /// ...when the static evaluation plus this...
    pub futility_base: i32,
    /// ...plus this much per ply of depth is still at or below alpha.
    pub futility_per_depth: i32,
    /// SEE pruning applies up to this depth...
    pub see_max_depth: u32,
    /// ...to quiet moves losing more than this much per ply of depth...
    pub see_quiet_margin: i32,
    /// ...and to captures losing more than this much per ply of depth.
    pub see_capture_margin: i32,
    /// Delta pruning: a capture is skipped if winning the piece plus this margin still
    /// leaves the score at or below alpha.
    pub delta_margin: i32,
    /// Internal iterative reductions apply from this depth on.
    pub iir_min_depth: u32,
    /// Singular extensions apply from this depth on...
    pub singular_min_depth: u32,
    /// ...when the hash move's entry is at most this many plies shallower than the node...
    pub singular_tt_depth_margin: u32,
    /// ...and no other move comes within this many centipawns per ply of depth of its score.
    pub singular_margin: i32,
    /// A singular hash move is searched two plies deeper if no other move comes within this
    /// many centipawns of `singular_beta` either (a double extension)...
    pub double_extension_margin: i32,
    /// ...at most this many times along a line.
    pub double_extension_limit: u32,
    /// A hash move that isn't singular but whose hash score beats beta is searched this many
    /// plies shallower (a negative extension).
    pub negative_extension_plies: u32,
    /// Correction history moves an entry towards a node's error by at most this many 256ths of
    /// the way (`depth + 1` below it)...
    pub correction_max_weight: i32,
    /// ...and no entry corrects by more than this many centipawns either way.
    pub correction_limit: i32,
    /// ProbCut applies from this depth on...
    pub probcut_min_depth: u32,
    /// ...and asks whether a capture beats beta by this many centipawns...
    pub probcut_margin: i32,
    /// ...in a search this many plies shallower.
    pub probcut_reduction: u32,
}

impl Default for SearchParams {
    fn default() -> SearchParams {
        SearchParams {
            aspiration_delta: 25,
            aspiration_min_depth: 5,
            aspiration_max_delta: 800,
            null_min_depth: 3,
            null_reduction: 3,
            null_depth_divisor: 6,
            lmr_min_depth: 3,
            lmr_min_moves: 3,
            lmr_base: 75,
            lmr_divisor: 225,
            // About one reduced move in seven has a quiet-move score of at least 128 either way
            // (measured over a self-play game; see the module documentation). The first
            // version used 256 and history alone, and changed about one in ten.
            lmr_history_divisor: 128,
            lmr_history_max: 2,
            lmp_max_depth: 3,
            lmp_base: 3,
            // Doubling it when improving was part of both failed versions (#60, #63).
            lmp_improving: 1,
            rfp_max_depth: 7,
            rfp_margin: 80,
            rfp_improving: 40,
            razor_max_depth: 2,
            razor_margin: 300,
            futility_max_depth: 6,
            futility_base: 100,
            futility_per_depth: 100,
            see_max_depth: 6,
            see_quiet_margin: 60,
            see_capture_margin: 100,
            delta_margin: 200,
            iir_min_depth: 4,
            // The first version started at 8 (cjus/chessengine#71); 6 gained +5.0 ± 4.1
            // (cjus/chessengine#78); 4 gained +6.1 ± 4.8 more (cjus/chessengine#80).
            singular_min_depth: 4,
            singular_tt_depth_margin: 3,
            // 2 until 4 became the start depth; 3 gained +5.2 ± 4.2 (cjus/chessengine#82); 4
            // failed, −1.8 ± 5.3 against 3 (cjus/chessengine#84).
            singular_margin: 3,
            double_extension_margin: 20,
            double_extension_limit: 6,
            negative_extension_plies: 1,
            correction_max_weight: 16,
            correction_limit: 64,
            // Set by hand, not tuned: a pawn above beta, four plies shallower.
            probcut_min_depth: 5,
            probcut_margin: 100,
            probcut_reduction: 4,
        }
    }
}
// ANCHOR_END: search_params

impl SearchParams {
    /// How many plies less to reduce a late quiet move with this history score: positive
    /// for a good history, negative for a bad one, at most `lmr_history_max` either way.
    pub fn lmr_history_adjustment(&self, history: i32) -> i32 {
        (history / self.lmr_history_divisor).clamp(-self.lmr_history_max, self.lmr_history_max)
    }
}

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
    /// Transposition table buckets: a key picks a bucket of slots that fills one cache line,
    /// and a store makes room by replacing the bucket's least valuable entry. Off, a key picks
    /// a single slot, as in stage 8 (see [`crate::tt`]).
    pub tt_buckets: bool,
    /// Killer moves: quiet moves that caused a cutoff at the same ply are tried early.
    pub killers: bool,
    /// The countermove heuristic: the quiet move that last refuted the opponent's move is
    /// tried early.
    pub countermoves: bool,
    /// The history heuristic: quiet moves are ordered by how often they caused cutoffs.
    pub history: bool,
    /// Continuation history: quiet moves are also ordered by how often they caused cutoffs
    /// after the moves one and two plies before.
    pub continuation_history: bool,
    /// Capture history: captures and queen promotions are ordered by how often they caused
    /// cutoffs as well as by MVV-LVA (needs MVV-LVA ordering; off by default).
    pub capture_history: bool,
    /// Principal variation search: moves after the first get a null-window search first.
    pub pvs: bool,
    /// Aspiration windows: each iteration starts with a narrow window around the last score.
    pub aspiration: bool,
    /// Null-move pruning: cut off when passing still keeps the score above beta. Only at
    /// null-window nodes, so it needs PVS.
    pub null_move: bool,
    /// Late move reductions: search late quiet moves less deeply (needs PVS).
    pub lmr: bool,
    /// Adjust late move reductions by history: reduce quiet moves with a good history less.
    pub history_lmr: bool,
    /// Late move pruning: skip late quiet moves near the leaves.
    pub late_move_pruning: bool,
    /// Reverse futility pruning (static null move).
    pub reverse_futility: bool,
    /// Razoring: drop into the quiescence search when far below alpha.
    pub razoring: bool,
    /// Futility pruning of quiet moves near the leaves.
    pub futility: bool,
    /// SEE pruning: skip moves that lose material, in the main and quiescence searches.
    pub see_pruning: bool,
    /// Delta pruning in the quiescence search.
    pub delta_pruning: bool,
    /// Order captures that lose material (by SEE) after the quiet moves.
    pub see_ordering: bool,
    /// Search moves that give check one ply deeper (off by default).
    pub check_extension: bool,
    /// Singular extensions: search the hash move a ply deeper when no other move comes close.
    pub singular_extension: bool,
    /// Double extensions: search a singular hash move two plies deeper when no other move
    /// comes close by a wide margin (needs singular extensions).
    pub double_extension: bool,
    /// Multi-cut: when the singular extension's search finds another move that beats beta
    /// too, cut the node off (needs singular extensions).
    pub multi_cut: bool,
    /// Negative extensions: search a hash move that isn't singular but beats beta a ply
    /// shallower (needs singular extensions).
    pub negative_extension: bool,
    /// Internal iterative reductions: search nodes without a hash move a ply shallower.
    pub iir: bool,
    /// Adjust pruning when the side to move is improving (better than two plies ago).
    pub improving: bool,
    /// Correction history: correct the static evaluation by how far off it has been in
    /// positions with the same pawns (see [`crate::correction`]).
    pub correction_history: bool,
    /// ProbCut: cut a node off when a good capture beats beta by a margin in a shallower
    /// search (needs the quiescence search; off by default).
    pub probcut: bool,
}

impl Default for Techniques {
    fn default() -> Techniques {
        Techniques {
            alpha_beta: true,
            quiescence: true,
            mvv_lva: true,
            tt: true,
            // Off by default: it measured weaker, −8.8 ± 8.6 Elo at 8+0.08 (cjus/chessengine#86),
            // and about even at 40+0.4, +1.6 ± 6.3 with no verdict (cjus/chessengine#92); see
            // chapter 8.1.
            tt_buckets: false,
            killers: true,
            countermoves: true,
            history: true,
            continuation_history: true,
            // Off by default: it measured weaker, −19.2 ± 11.9 Elo after 1,561 games at 8+0.08
            // (cjus/chessengine#115), though it took 11.5% off the bench; see chapter 10.2.
            capture_history: false,
            pvs: true,
            aspiration: true,
            null_move: true,
            lmr: true,
            // The second version passed its SPRT (cjus/chessengine#74); the first had no verdict.
            history_lmr: true,
            late_move_pruning: true,
            reverse_futility: true,
            razoring: true,
            futility: true,
            see_pruning: true,
            delta_pruning: true,
            see_ordering: true,
            // Off by default: it didn't measure stronger (see the module documentation).
            check_extension: false,
            singular_extension: true,
            // Off by default: no verdict, +3.6 ± 4.9 Elo after 8,670 games at 8+0.08
            // (cjus/chessengine#96); see chapter 14.1.
            double_extension: false,
            multi_cut: true,
            negative_extension: true,
            iir: true,
            // Off by default: three versions measured weaker (see the module documentation).
            improving: false,
            correction_history: true,
            // Off by default: it measured weaker, −10.2 ± 9.1 Elo after 2,593 games at 8+0.08
            // (cjus/chessengine#117), though it took 9.6% off the bench; see chapter 12.1.
            probcut: false,
        }
    }
}

impl Techniques {
    /// Only the techniques that don't change the search's result, just its speed (alpha-beta,
    /// quiescence, move ordering, the transposition table and PVS), with the selective ones
    /// (null move, reductions and every kind of pruning) off. Useful for comparing node counts
    /// at equal scores. (The transposition table can still change a result now and then, when
    /// it supplies a deeper result than the search asked for.)
    pub fn sound() -> Techniques {
        Techniques {
            null_move: false,
            lmr: false,
            history_lmr: false,
            late_move_pruning: false,
            reverse_futility: false,
            razoring: false,
            futility: false,
            see_pruning: false,
            delta_pruning: false,
            check_extension: false,
            singular_extension: false,
            double_extension: false,
            multi_cut: false,
            negative_extension: false,
            iir: false,
            improving: false,
            correction_history: false,
            probcut: false,
            ..Techniques::default()
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
    tt: &'a TranspositionTable,
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
    /// The moves from the root to the current position (for the countermove heuristic), with
    /// `None` for a null move.
    path: Vec<Option<Move>>,
    /// The same moves by piece and destination, for continuation history (`None` for a null
    /// move). Kept in step with `path` by [`Searcher::push_move`] and [`Searcher::pop_move`].
    moved: Vec<Option<PieceTo>>,
    /// The static evaluation at each ply of the current line (`-INFINITY` in check), for the
    /// improving heuristic.
    evals: Vec<i32>,
    /// A move to leave out at each ply: the hash move, while a singular extension's search
    /// asks whether any other move comes close to it.
    excluded: Vec<Option<Move>>,
    /// The depth of the current iteration, which limits how far extensions can reach.
    root_depth: u32,
    /// Double extensions on the line from the root to the current node, which
    /// `double_extension_limit` limits.
    double_extensions: u32,
    /// Late move reductions by depth and move number (see [`lmr_table`]).
    lmr: Vec<[u8; LMR_TABLE_SIZE]>,
    /// A counter shared with other threads, which this search adds its nodes to.
    shared_nodes: Option<&'a AtomicU64>,
    /// Endgame tablebases to probe, and when.
    tablebase: Option<(&'a dyn Tablebase, TablebaseParams)>,
    /// Tablebase probes that gave a result.
    tb_hits: u64,
    /// The engine's correction history, if it gave this search one (see
    /// [`Searcher::use_correction`]).
    correction: Option<&'a mut CorrectionHistory>,
    /// What the search has done so far (see [`crate::stats`]).
    stats: SearchStats,
    /// The search tree, when one is being recorded.
    tree: Option<SearchTree>,
}

/// The late move reduction table covers depths and move numbers below this.
const LMR_TABLE_SIZE: usize = 64;

// ANCHOR: lmr_table
/// The late move reduction for each depth and move number:
/// `base + ln(depth) × ln(move number) / divisor`, rounded down.
fn lmr_table(p: &SearchParams) -> Vec<[u8; LMR_TABLE_SIZE]> {
    let base = f64::from(p.lmr_base) / 100.0;
    let divisor = f64::from(p.lmr_divisor) / 100.0;
    (0..LMR_TABLE_SIZE)
        .map(|depth| {
            let mut row = [0; LMR_TABLE_SIZE];
            for (index, r) in row.iter_mut().enumerate().skip(1) {
                if depth > 0 {
                    let value = base + (depth as f64).ln() * (index as f64).ln() / divisor;
                    *r = value.max(0.0) as u8;
                }
            }
            row
        })
        .collect()
}
// ANCHOR_END: lmr_table

/// How [`Searcher::search_move`] searches a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MoveSearch {
    /// The first move: full window, full depth.
    First,
    /// A later move: null window (PVS), reduced by this many plies first (LMR).
    Later {
        /// The late move reduction, in plies.
        reduction: u32,
    },
}

impl<'a> Searcher<'a> {
    pub(crate) fn new(
        params: &'a EvalParams,
        search_params: SearchParams,
        tt: &'a TranspositionTable,
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
            moved: Vec::new(),
            evals: vec![-INFINITY; MAX_PLY as usize + 1],
            excluded: vec![None; MAX_PLY as usize + 1],
            root_depth: 0,
            double_extensions: 0,
            lmr: lmr_table(&search_params),
            shared_nodes: None,
            tablebase: None,
            tb_hits: 0,
            correction: None,
            stats: SearchStats::default(),
            tree: None,
        }
    }

    /// Corrects the static evaluation with `correction`, and updates it, from now on (if
    /// [`Techniques::correction_history`] is on).
    pub(crate) fn use_correction(&mut self, correction: &'a mut CorrectionHistory) {
        self.correction = Some(correction);
    }

    /// `raw_eval`, the static evaluation of `pos`, corrected by the correction history if
    /// there is one and it is switched on.
    fn corrected_eval(&self, pos: &Position, raw_eval: i32) -> i32 {
        match self.correction.as_deref() {
            Some(correction) if self.techniques.correction_history => {
                correction.corrected(pos, raw_eval)
            }
            _ => raw_eval,
        }
    }

    /// Records the search's decisions in `tree` from now on.
    pub(crate) fn record_tree(&mut self, tree: SearchTree) {
        self.tree = Some(tree);
    }

    /// The recorded search tree, if one was being recorded.
    pub(crate) fn take_tree(&mut self) -> Option<SearchTree> {
        self.tree.take()
    }

    /// What the search has done so far.
    pub(crate) fn stats(&self) -> SearchStats {
        self.stats
    }

    /// Reports a decision to the statistics and, when one is being recorded, the search tree.
    fn note(&mut self, note: Note) {
        self.stats.count(note);
        if let Some(tree) = &mut self.tree {
            tree.record(&self.path, note);
        }
    }

    /// Probes `tb` during the search, as `params` allow.
    pub(crate) fn use_tablebase(&mut self, tb: &'a dyn Tablebase, params: TablebaseParams) {
        self.tablebase = Some((tb, params));
    }

    /// Tablebase probes that gave a result so far.
    pub(crate) fn tb_hits(&self) -> u64 {
        self.tb_hits
    }

    /// Adds this search's node count to `counter` as it goes (helper threads report their
    /// nodes this way, so the main thread can show the total).
    pub(crate) fn count_nodes_in(&mut self, counter: &'a AtomicU64) {
        self.shared_nodes = Some(counter);
    }

    /// Whether the search must end now (stop flag, time or node limit). The stop flag and
    /// the clock are checked every [`CHECK_INTERVAL`] nodes, since reading the clock isn't
    /// free.
    fn should_stop(&mut self) -> bool {
        if self.aborted {
            return true;
        }
        // The node limit is checked at every node: it's free, and strength levels use small
        // limits (a few hundred nodes) that must be exact.
        if self.max_nodes.is_some_and(|n| self.nodes >= n) {
            self.aborted = true;
        } else if self.nodes.is_multiple_of(CHECK_INTERVAL) {
            if let Some(counter) = self.shared_nodes {
                counter.fetch_add(CHECK_INTERVAL, Ordering::Relaxed);
            }
            self.aborted = self.stop.load(Ordering::Relaxed)
                || self.deadline.is_some_and(|d| Instant::now() >= d);
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
        self.note(Note::Enter { depth, alpha, beta });
        self.seldepth = self.seldepth.max(ply);
        pv.clear();
        if self.should_stop() {
            return 0;
        }

        // Set while a singular extension's search leaves this node's hash move out.
        let excluded = self.excluded[ply as usize];
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
        // (Not in a singular extension's search: the entry is about the node with every move.)
        let mut tt_move = None;
        let mut tt_entry = None;
        if self.techniques.tt
            && excluded.is_none()
            && let Some(entry) = self.tt.probe(pos.hash())
        {
            self.note(Note::TtHit);
            tt_entry = Some(entry);
            tt_move = entry.best_move.filter(|mv| moves.contains(mv));
            if u32::from(entry.depth) >= depth {
                let score = score_from_tt(entry.score, ply);
                let usable = match entry.bound {
                    Bound::Exact => true,
                    Bound::Lower => score >= beta,
                    Bound::Upper => score <= alpha,
                };
                if usable {
                    self.note(Note::TtCutoff);
                    pv.extend(tt_move);
                    return score;
                }
            }
        }
        // ANCHOR_END: tt_probe

        // ANCHOR: iir
        // Internal iterative reduction: with no hash move to try first, move ordering is poor
        // and the node probably isn't important; search it a ply shallower.
        let depth = if self.techniques.iir
            && tt_move.is_none()
            && excluded.is_none()
            && depth >= self.search_params.iir_min_depth
        {
            depth - 1
        } else {
            depth
        };
        // ANCHOR_END: iir

        // ANCHOR: tb_probe
        // Endgame tablebases: right after a capture or pawn move, a covered position's result
        // is known exactly.
        if let Some((tb, tb_params)) = self.tablebase
            && ply > 0
            && depth >= tb_params.probe_depth
            && pos.halfmove_clock() == 0
            && tablebase::covers(tb, &tb_params, pos)
            && let Some(wdl) = tb.probe_wdl(pos)
        {
            self.tb_hits += 1;
            self.note(Note::Tablebase);
            let score = tablebase::score(wdl, ply);
            if self.techniques.tt {
                let stored = score_to_tt(score, ply);
                self.tt
                    .store(pos.hash(), None, stored, MAX_PLY, Bound::Exact);
            }
            return score;
        }
        // ANCHOR_END: tb_probe

        let p = self.search_params;
        let in_check = pos.in_check();
        let pv_node = beta - alpha > 1;
        // ANCHOR: static_eval
        // The static evaluation, for the pruning decisions (meaningless in check), corrected by
        // what the search has learnt about positions with these pawns.
        let raw_eval = if in_check {
            -INFINITY
        } else {
            evaluate(pos, self.params)
        };
        let static_eval = if in_check {
            -INFINITY
        } else {
            self.corrected_eval(pos, raw_eval)
        };
        // ANCHOR_END: static_eval
        // ANCHOR: improving
        // Improving: better than two plies ago, when this side last moved. Its evaluation will
        // probably keep rising, which changes the bets the pruning techniques make.
        self.evals[ply as usize] = static_eval;
        let improving = self.techniques.improving
            && !in_check
            && ply >= 2
            && static_eval > self.evals[ply as usize - 2];
        // ANCHOR_END: improving
        // Selective pruning happens only off the principal variation and out of check.
        let selective = self.techniques.alpha_beta && !pv_node && !in_check;

        // ANCHOR: reverse_futility
        // Reverse futility pruning: far enough above beta, assume a search would stay there.
        // An improving side gets a smaller margin, but never none at all.
        let rfp_margin = p.rfp_margin * depth as i32 - if improving { p.rfp_improving } else { 0 };
        if self.techniques.reverse_futility
            && selective
            && depth <= p.rfp_max_depth
            && static_eval - rfp_margin >= beta
        {
            self.note(Note::ReverseFutility);
            return static_eval;
        }
        // ANCHOR_END: reverse_futility

        // ANCHOR: razoring
        // Razoring: far below alpha near the leaves, only captures could save the node.
        if self.techniques.razoring
            && self.techniques.quiescence
            && selective
            && depth <= p.razor_max_depth
            && static_eval + p.razor_margin * (depth as i32) < alpha
        {
            let score = self.quiescence(pos, ply, alpha, alpha + 1, pv);
            if score <= alpha {
                self.note(Note::Razoring);
                return score;
            }
            pv.clear();
        }
        // ANCHOR_END: razoring

        // ANCHOR: null_move
        // Null-move pruning: if passing still keeps us at or above beta, a real move would too.
        if self.techniques.null_move
            && selective
            && excluded.is_none()
            && depth >= p.null_min_depth
            && self.path.last() != Some(&None)
            && has_pieces(pos)
            && static_eval >= beta
        {
            self.note(Note::NullMove);
            let reduction = p.null_reduction + depth / p.null_depth_divisor;
            let mut child = pos.clone();
            child.make_null_move();
            self.push_move(pos, None);
            self.keys.push(pos.hash());
            let mut null_pv = Vec::new();
            let score = -self.negamax(
                &child,
                depth.saturating_sub(1 + reduction),
                ply + 1,
                -beta,
                -beta + 1,
                &mut null_pv,
            );
            self.note(Note::Exit { score: -score });
            self.keys.pop();
            self.pop_move();
            if self.aborted {
                return 0;
            }
            if score >= beta {
                self.note(Note::NullMoveCutoff);
                // A mate found after passing isn't a real mate: claim no more than beta.
                return if score >= MATE - MAX_PLY as i32 {
                    beta
                } else {
                    score
                };
            }
        }
        // ANCHOR_END: null_move

        // ANCHOR: probcut
        // ProbCut: if a good capture beats beta by a margin even in a shallower search, the
        // full-depth search would almost surely beat beta too, so the node is cut off. Not if
        // the hash table already says that a search about as deep fell short of the margin.
        let probcut_beta = beta + p.probcut_margin;
        if self.techniques.probcut
            && self.techniques.quiescence
            && selective
            && excluded.is_none()
            && depth >= p.probcut_min_depth
            && beta.abs() + p.probcut_margin < MATE - MAX_PLY as i32
            && !tt_entry.is_some_and(|entry| {
                entry.bound != Bound::Lower
                    && u32::from(entry.depth) + p.probcut_reduction > depth
                    && score_from_tt(entry.score, ply) < probcut_beta
            })
        {
            self.note(Note::ProbCut);
            let probcut_depth = depth.saturating_sub(p.probcut_reduction);
            let captures: Vec<Move> = moves.iter().copied().filter(|&mv| is_noisy(mv)).collect();
            let previous = self.previous();
            let picker = MovePicker::new(
                pos,
                &captures,
                tt_move,
                ply,
                &previous,
                self.ordering,
                &self.techniques,
            );
            let mut cutoff = None;
            let mut probcut_pv = Vec::new();
            self.keys.push(pos.hash());
            for mv in picker {
                // Only captures that win, by static exchange, at least what the static
                // evaluation is short of the margin.
                if see(pos, mv) < probcut_beta - static_eval {
                    continue;
                }
                let mut child = pos.clone();
                child.make_move(mv);
                self.push_move(pos, Some(mv));
                // The quiescence search first: it is cheap, and if even it can't reach the
                // margin, the shallower search seldom does.
                let mut score = -self.quiescence(
                    &child,
                    ply + 1,
                    -probcut_beta,
                    -probcut_beta + 1,
                    &mut probcut_pv,
                );
                if score >= probcut_beta && probcut_depth > 0 && !self.aborted {
                    score = self.search_child(
                        &child,
                        probcut_depth,
                        probcut_beta - 1,
                        probcut_beta,
                        &mut probcut_pv,
                    );
                }
                self.pop_move();
                if self.aborted {
                    break;
                }
                if score >= probcut_beta {
                    cutoff = Some((mv, score));
                    break;
                }
            }
            self.keys.pop();
            if self.aborted {
                return 0;
            }
            if let Some((mv, score)) = cutoff {
                self.note(Note::ProbCutCutoff(mv));
                // What ProbCut found is a lower bound from its shallower search.
                if self.techniques.tt {
                    let stored = score_to_tt(score, ply);
                    self.tt.store(
                        pos.hash(),
                        Some(mv),
                        stored,
                        probcut_depth + 1,
                        Bound::Lower,
                    );
                }
                return score;
            }
        }
        // ANCHOR_END: probcut

        // ANCHOR: singular
        // Singular extension: is the hash move much better than every other move? Search the
        // node again, at half the depth and without the hash move, against a window a little
        // below the hash move's score. If no other move reaches it, the hash move is probably
        // forced, and it is searched a ply deeper.
        let mut singular_move = None;
        let mut double_move = None;
        let mut shallower_move = None;
        if self.techniques.singular_extension
            && depth >= p.singular_min_depth
            && ply < 2 * self.root_depth
            && let Some(entry) = tt_entry
            && let Some(hash_move) = tt_move
            && entry.bound != Bound::Upper
            && u32::from(entry.depth) + p.singular_tt_depth_margin >= depth
        {
            let tt_score = score_from_tt(entry.score, ply);
            if tt_score.abs() < MATE - MAX_PLY as i32 {
                let singular_beta = tt_score - p.singular_margin * depth as i32;
                self.note(Note::SingularSearch(hash_move));
                self.excluded[ply as usize] = Some(hash_move);
                let mut singular_pv = Vec::new();
                let score = self.negamax(
                    pos,
                    (depth - 1) / 2,
                    ply,
                    singular_beta - 1,
                    singular_beta,
                    &mut singular_pv,
                );
                self.excluded[ply as usize] = None;
                self.note(Note::Exit { score });
                if self.aborted {
                    return 0;
                }
                if score < singular_beta {
                    self.note(Note::SingularExtension(hash_move));
                    singular_move = Some(hash_move);
                    // Double extension: nothing else comes close even well below
                    // `singular_beta`, so the hash move is searched a second ply deeper. Only
                    // off the principal variation, and only a few times along a line, so that
                    // the search can't explode.
                    if self.techniques.double_extension
                        && !pv_node
                        && score < singular_beta - p.double_extension_margin
                        && self.double_extensions < p.double_extension_limit
                    {
                        self.note(Note::DoubleExtension(hash_move));
                        double_move = Some(hash_move);
                    }
                } else if self.techniques.multi_cut && singular_beta >= beta {
                    // Multi-cut: the hash move beat beta, and another move beats it too even
                    // at half the depth. With two moves that good, the node is a cutoff.
                    self.note(Note::MultiCut);
                    return singular_beta;
                } else if self.techniques.negative_extension && tt_score >= beta {
                    // Negative extension: not singular, but its hash score beats beta. The
                    // node will probably cut off either way, so its line needs less care.
                    self.note(Note::NegativeExtension(hash_move));
                    shallower_move = Some(hash_move);
                }
            }
        }
        // ANCHOR_END: singular

        // In a singular extension's search, the hash move is left out. If it was the only
        // move, nothing else comes close: fail low.
        let without_excluded: Vec<Move>;
        let moves: &[Move] = match excluded {
            Some(left_out) => {
                without_excluded = moves.iter().copied().filter(|&mv| mv != left_out).collect();
                &without_excluded
            }
            None => &moves,
        };
        if moves.is_empty() {
            return alpha;
        }

        let previous = self.previous();
        let picker = MovePicker::new(
            pos,
            moves,
            tt_move,
            ply,
            &previous,
            self.ordering,
            &self.techniques,
        );
        let original_alpha = alpha;
        let mut best = -INFINITY;
        let mut best_move = None;
        let mut child_pv = Vec::new();
        // Quiet moves searched so far that didn't cause a cutoff (for the history penalty),
        // and the same for captures and queen promotions (for capture history's).
        let mut quiets_tried = Vec::new();
        let mut noisy_tried = Vec::new();
        // Moves actually searched (not pruned) so far.
        let mut searched = 0;
        self.keys.push(pos.hash());
        for (index, mv) in picker.enumerate() {
            let quiet = !is_noisy(mv);
            let mut child = pos.clone();
            child.make_move(mv);
            let gives_check = child.in_check();
            let ordinary = quiet && !in_check && !gives_check;

            // ANCHOR: lmp
            // Late move pruning: near the leaves, after enough quiet moves, skip the rest
            // (once a move has shown we aren't simply getting mated).
            if self.techniques.late_move_pruning
                && ordinary
                && !pv_node
                && depth <= p.lmp_max_depth
                && best > -MATE + MAX_PLY as i32
                && quiets_tried.len() as u32
                    >= (p.lmp_base + depth * depth) * if improving { p.lmp_improving } else { 1 }
            {
                self.note(Note::LateMovePruning(mv));
                continue;
            }
            // ANCHOR_END: lmp

            // ANCHOR: futility
            // Futility pruning: a quiet move can't lift a hopeless static evaluation to alpha.
            if self.techniques.futility
                && ordinary
                && !pv_node
                && depth <= p.futility_max_depth
                && best > -MATE + MAX_PLY as i32
                && static_eval + p.futility_base + p.futility_per_depth * depth as i32 <= alpha
            {
                self.note(Note::Futility(mv));
                continue;
            }
            // ANCHOR_END: futility

            // ANCHOR: see_pruning
            // SEE pruning: near the leaves, skip moves that lose material outright.
            if self.techniques.see_pruning
                && selective
                && index > 0
                && !gives_check
                && depth <= p.see_max_depth
                && best > -MATE + MAX_PLY as i32
            {
                let margin = if quiet {
                    p.see_quiet_margin
                } else {
                    p.see_capture_margin
                };
                if see(pos, mv) < -margin * (depth as i32) {
                    self.note(Note::SeePruning(mv));
                    continue;
                }
            }
            // ANCHOR_END: see_pruning

            // ANCHOR: lmr
            let how = if index == 0 {
                MoveSearch::First
            } else if self.techniques.lmr
                && ordinary
                && depth >= p.lmr_min_depth
                && index as u32 >= p.lmr_min_moves
            {
                let row = &self.lmr[(depth as usize).min(LMR_TABLE_SIZE - 1)];
                let mut reduction = i32::from(row[index.min(LMR_TABLE_SIZE - 1)]);
                // Reduce the principal variation less: its exact score matters.
                if pv_node {
                    reduction -= 1;
                }
                // Reduce a move with a good history less: it has caused cutoffs elsewhere. And
                // one with a bad history more.
                if self.techniques.history_lmr {
                    let score = self
                        .ordering
                        .quiet_score(pos, mv, &previous, &self.techniques);
                    reduction -= p.lmr_history_adjustment(score);
                }
                MoveSearch::Later {
                    // Never reduce into the quiescence search.
                    reduction: reduction.clamp(0, depth as i32 - 2) as u32,
                }
            } else {
                MoveSearch::Later { reduction: 0 }
            };
            // ANCHOR_END: lmr

            // ANCHOR: check_extension
            // Check extension: a move that gives check is searched one ply deeper. So is a
            // singular hash move (above), and one singular by a wide margin a ply more.
            let doubled = Some(mv) == double_move;
            let extension = u32::from(
                (self.techniques.check_extension && gives_check && ply < MAX_PLY / 2)
                    || Some(mv) == singular_move,
            ) + u32::from(doubled);
            // A hash move that isn't singular but beats beta is searched shallower (above).
            let shallower = if Some(mv) == shallower_move {
                p.negative_extension_plies
            } else {
                0
            };
            // ANCHOR_END: check_extension

            searched += 1;
            self.push_move(pos, Some(mv));
            let child_depth = (depth - 1 + extension).saturating_sub(shallower);
            self.double_extensions += u32::from(doubled);
            let score = self.search_move(&child, child_depth, alpha, beta, how, &mut child_pv);
            self.double_extensions -= u32::from(doubled);
            self.pop_move();
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
            // ANCHOR: cutoff
            // Beta cutoff: the opponent won't allow this position, so stop looking.
            if self.techniques.alpha_beta && alpha >= beta {
                self.note(Note::BetaCutoff(mv, searched));
                if quiet {
                    self.ordering
                        .record_cutoff(pos, mv, &quiets_tried, ply, depth, &previous);
                }
                // The captures tried before it failed, whichever kind of move cut off.
                let noisy_best = (!quiet).then_some(mv);
                self.ordering
                    .record_captures(pos, noisy_best, &noisy_tried, depth);
                break;
            }
            if quiet {
                quiets_tried.push(mv);
            } else {
                noisy_tried.push(mv);
            }
            // ANCHOR_END: cutoff
        }
        self.keys.pop();

        let bound = if best >= beta {
            Bound::Lower
        } else if best > original_alpha {
            Bound::Exact
        } else {
            Bound::Upper
        };
        // ANCHOR: tt_store
        // (Not in a singular extension's search: its result leaves the best move out.)
        if self.techniques.tt && !self.aborted && excluded.is_none() {
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

        // ANCHOR: correction_update
        // Correction history learns how far the static evaluation was off: not in check (there
        // is none), not in a singular extension's search (its score leaves the best move out),
        // and not when the best move is a capture or promotion (its gain is material the static
        // evaluation isn't meant to see).
        if self.techniques.correction_history
            && !self.aborted
            && !in_check
            && excluded.is_none()
            && !best_move.is_some_and(is_noisy)
            && let Some(correction) = self.correction.as_deref_mut()
        {
            correction.update(pos, depth, raw_eval, best, bound, &self.search_params);
        }
        // ANCHOR_END: correction_update
        best
    }
    // ANCHOR_END: negamax

    /// Adds `mv`, about to be played in `pos` (`None` for a null move), to the path.
    fn push_move(&mut self, pos: &Position, mv: Option<Move>) {
        self.path.push(mv);
        self.moved.push(mv.and_then(|mv| PieceTo::new(pos, mv)));
    }

    /// Takes the last move off the path.
    fn pop_move(&mut self) {
        self.path.pop();
        self.moved.pop();
    }

    /// The moves that led to the current position, for move ordering.
    fn previous(&self) -> Previous {
        let mut moved = [None; CONTINUATION_PLIES];
        for (slot, earlier) in moved.iter_mut().zip(self.moved.iter().rev()) {
            *slot = *earlier;
        }
        Previous {
            mv: self.path.last().copied().flatten(),
            moved,
        }
    }

    // ANCHOR: pvs
    /// The score of the move that led to `child` (the last move on the path), from the
    /// mover's side, searched to `depth`.
    ///
    /// The first move gets the full window. With PVS on, a later move gets a null window
    /// first, at a reduced depth if LMR chose a reduction; if it beats `alpha` it is searched
    /// again at full depth, and then with the full window if it is inside it.
    fn search_move(
        &mut self,
        child: &Position,
        depth: u32,
        alpha: i32,
        beta: i32,
        how: MoveSearch,
        pv: &mut Vec<Move>,
    ) -> i32 {
        let pvs = self.techniques.pvs && self.techniques.alpha_beta;
        let reduction = match how {
            MoveSearch::Later { reduction } if pvs => reduction,
            _ => return self.search_child(child, depth, alpha, beta, pv),
        };
        let mv = self.path.last().copied().flatten();
        if reduction > 0
            && let Some(mv) = mv
        {
            self.note(Note::Reduced(mv, reduction));
        }
        let mut score = self.search_child(child, depth - reduction, alpha, alpha + 1, pv);
        if score > alpha && reduction > 0 && !self.aborted {
            if let Some(mv) = mv {
                self.note(Note::ReductionResearch(mv));
            }
            score = self.search_child(child, depth, alpha, alpha + 1, pv);
        }
        if score > alpha && score < beta && !self.aborted {
            if let Some(mv) = mv {
                self.note(Note::PvsResearch(mv));
            }
            score = self.search_child(child, depth, alpha, beta, pv);
        }
        score
    }

    /// The score of `child` for the side that moved there, searched to `depth` within the
    /// mover's window `[alpha, beta]`.
    fn search_child(
        &mut self,
        child: &Position,
        depth: u32,
        alpha: i32,
        beta: i32,
        pv: &mut Vec<Move>,
    ) -> i32 {
        let ply = self.path.len() as u32;
        let score = -self.negamax(child, depth, ply, -beta, -alpha, pv);
        self.note(Note::Exit { score: -score });
        score
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
        self.stats.count(Note::Quiescence);
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
            // Stand pat: not capturing is an option, so the static score is a lower bound. It
            // is corrected, as in the main search.
            best = self.corrected_eval(pos, evaluate(pos, self.params));
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
            &Previous::default(),
            self.ordering,
            &self.techniques,
        );
        let stand_pat = best;
        let mut child_pv = Vec::new();
        for mv in picker {
            if !in_check {
                // Delta pruning: even winning the piece outright wouldn't reach alpha.
                if self.techniques.delta_pruning && mv.promotion().is_none() {
                    let gain = pos
                        .piece_at(mv.to())
                        .map_or(SEE_VALUES[0], |p| SEE_VALUES[p.piece_type.index()]);
                    if stand_pat + gain + self.search_params.delta_margin <= alpha {
                        self.stats.count(Note::DeltaPruning(mv));
                        continue;
                    }
                }
                // SEE pruning: a capture that loses material can't help.
                if self.techniques.see_pruning && see(pos, mv) < 0 {
                    self.stats.count(Note::SeePruning(mv));
                    continue;
                }
            }
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
        self.root_depth = depth;
        // The root's static evaluation, for the improving heuristic two plies down. `negamax`
        // stores its own, but the root is searched here.
        self.evals[0] = if pos.in_check() {
            -INFINITY
        } else {
            evaluate(pos, self.params)
        };
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
            self.push_move(pos, Some(mv));
            let score = self.search_move(
                &child,
                depth.saturating_sub(1),
                alpha,
                beta,
                if index == 0 {
                    MoveSearch::First
                } else {
                    MoveSearch::Later { reduction: 0 }
                },
                &mut child_pv,
            );
            self.pop_move();
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

/// Whether the side to move has a piece other than pawns and its king: without one,
/// zugzwang is likely and null moves can't be trusted.
fn has_pieces(pos: &Position) -> bool {
    let us = pos.side_to_move();
    [
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ]
    .iter()
    .any(|&pt| !pos.pieces(us, pt).is_empty())
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
        let tt = TranspositionTable::new(1);
        let mut ordering = OrderingTables::default();
        let mut searcher = Searcher::new(
            &params,
            SearchParams::default(),
            &tt,
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
            ..Techniques::sound()
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
            continuation_history: false,
            ..Techniques::sound()
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
    fn capture_history_changes_the_order_not_the_score() {
        // Kiwipete and a middlegame with exchanges on the board: captures to order.
        let fens = [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1",
        ];
        let on = Techniques {
            capture_history: true,
            ..Techniques::sound()
        };
        for fen in fens {
            let with = search(fen, 5, on);
            let without = search(
                fen,
                5,
                Techniques {
                    capture_history: false,
                    ..on
                },
            );
            assert_eq!(with.score, without.score, "{fen}");
            assert_ne!(with.nodes, without.nodes, "{fen}");
        }
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
            continuation_history: false,
            ..Techniques::sound()
        };
        let (mut with_nodes, mut without_nodes) = (0, 0);
        for fen in fens {
            let with = search(fen, 4, Techniques::sound());
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
        // Several positions: in one position at one depth, PVS can cost a few nodes (the first
        // of these did by 1% at depth 5 once continuation history came in).
        let fens = [
            "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        ];
        let (mut with_nodes, mut without_nodes) = (0, 0);
        for fen in fens {
            let with = search(fen, 5, Techniques::sound());
            let without = search(
                fen,
                5,
                Techniques {
                    pvs: false,
                    ..Techniques::sound()
                },
            );
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
    fn null_move_pruning_saves_nodes() {
        let fens = [
            "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1",
            "r2q1rk1/pp2bppp/2n1pn2/3p4/3P4/2NBPN2/PP3PPP/R2Q1RK1 w - - 0 10",
        ];
        let off = Techniques {
            null_move: false,
            ..Techniques::default()
        };
        for fen in fens {
            let with = search(fen, 6, Techniques::default());
            let without = search(fen, 6, off);
            assert!(
                with.nodes < without.nodes,
                "{fen}: {} vs {}",
                with.nodes,
                without.nodes
            );
        }
    }

    #[test]
    fn no_null_moves_in_pawn_endgames() {
        // Zugzwang territory: with only pawns, the null move must not be tried, so switching
        // it off changes nothing.
        let fen = "8/8/1p6/1P1k4/3P4/3K4/8/8 w - - 0 1";
        let with = search(fen, 8, Techniques::default());
        let without = search(
            fen,
            8,
            Techniques {
                null_move: false,
                ..Techniques::default()
            },
        );
        assert_eq!(with.nodes, without.nodes);
        assert_eq!(with.score, without.score);
    }

    #[test]
    fn the_root_stores_its_static_evaluation_for_improving() {
        // Nodes two plies down compare their evaluation with the root's. cjus/chessengine#60
        // and #63 ran without it, so every one of them counted as improving.
        let pos = Position::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        let params = EvalParams::default();
        let stop = AtomicBool::new(false);
        let tt = TranspositionTable::new(1);
        let mut ordering = OrderingTables::default();
        let mut searcher = Searcher::new(
            &params,
            SearchParams::default(),
            &tt,
            &mut ordering,
            Techniques::default(),
            StopConditions {
                stop: &stop,
                deadline: None,
                max_nodes: None,
            },
            &[],
        );
        let mut moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        searcher.search_depth(&pos, &mut moves, 2, None);
        assert_eq!(searcher.evals[0], evaluate(&pos, &params));
    }

    #[test]
    fn the_reverse_futility_margin_never_reaches_zero() {
        // When improving, the margin at depth 1 is `rfp_margin - rfp_improving`. With no margin
        // at all, a static evaluation that only just reaches beta would cut the node off
        // (cjus/chessengine#60 measured that: −32 Elo).
        let p = SearchParams::default();
        assert!(p.rfp_improving >= 0);
        assert!(p.rfp_improving < p.rfp_margin);
    }

    #[test]
    fn lmr_table_grows_with_depth_and_move_number() {
        let table = lmr_table(&SearchParams::default());
        // 0.75 + ln(10) × ln(20) / 2.25 = 3.82: three plies.
        assert_eq!(table[10][20], 3);
        assert_eq!(table[3][3], 1);
        assert_eq!(table[1][40], 0);
        for depth in 1..LMR_TABLE_SIZE {
            for index in 2..LMR_TABLE_SIZE {
                assert!(table[depth][index] >= table[depth][index - 1]);
                assert!(table[depth][index] >= table[depth - 1][index]);
            }
        }
    }

    #[test]
    fn late_move_reductions_and_pruning_save_nodes_and_still_find_mates() {
        let fen = "r2q1rk1/pp2bppp/2n1pn2/3p4/3P4/2NBPN2/PP3PPP/R2Q1RK1 w - - 0 10";
        let both = search(fen, 6, Techniques::default());
        let neither = search(
            fen,
            6,
            Techniques {
                lmr: false,
                late_move_pruning: false,
                ..Techniques::default()
            },
        );
        assert!(
            both.nodes * 2 < neither.nodes,
            "{} vs {}",
            both.nodes,
            neither.nodes
        );
        // Pruning must not lose forced mates.
        let mate = search(
            "2r3k1/5ppp/8/8/8/8/3R1PPP/3R2K1 w - - 0 1",
            6,
            Techniques::default(),
        );
        assert_eq!(to_score(mate.score), Score::Mate(2));
    }

    #[test]
    fn stage_14_pruning_saves_nodes_and_still_finds_mates() {
        let fen = "r2q1rk1/pp2bppp/2n1pn2/3p4/3P4/2NBPN2/PP3PPP/R2Q1RK1 w - - 0 10";
        let on = search(fen, 6, Techniques::default());
        let off = search(
            fen,
            6,
            Techniques {
                reverse_futility: false,
                razoring: false,
                futility: false,
                see_pruning: false,
                delta_pruning: false,
                ..Techniques::default()
            },
        );
        assert!(on.nodes < off.nodes, "{} vs {}", on.nodes, off.nodes);
        let mate = search(
            "2r3k1/5ppp/8/8/8/8/3R1PPP/3R2K1 w - - 0 1",
            6,
            Techniques::default(),
        );
        assert_eq!(to_score(mate.score), Score::Mate(2));
        // The queen sacrifice is still seen past the horizon: Qxd5 loses the queen.
        let result = search(
            "4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1",
            3,
            Techniques::default(),
        );
        assert_ne!(result.best_move.unwrap().to_string(), "d1d5");
    }

    #[test]
    fn mate_scores_convert_to_moves() {
        assert_eq!(to_score(MATE - 1), Score::Mate(1));
        assert_eq!(to_score(MATE - 3), Score::Mate(2));
        assert_eq!(to_score(-MATE + 2), Score::Mate(-1));
        assert_eq!(to_score(57), Score::Centipawns(57));
    }

    #[test]
    fn history_adjusts_reductions_by_at_most_lmr_history_max() {
        let p = SearchParams::default();
        let step = p.lmr_history_divisor;
        assert_eq!(p.lmr_history_adjustment(0), 0);
        assert_eq!(p.lmr_history_adjustment(step - 1), 0);
        assert_eq!(p.lmr_history_adjustment(-(step - 1)), 0);
        assert_eq!(p.lmr_history_adjustment(step), 1);
        assert_eq!(p.lmr_history_adjustment(-step), -1);
        let max = crate::ordering::HISTORY_MAX;
        assert_eq!(p.lmr_history_adjustment(max), p.lmr_history_max);
        assert_eq!(p.lmr_history_adjustment(-max), -p.lmr_history_max);
    }

    #[test]
    fn a_search_without_the_only_move_fails_low_and_stores_nothing() {
        // Black has one legal move, ...a5: the queen on g6 covers g8, g7 and h7.
        let pos = Position::from_fen("7k/8/p5Q1/8/8/8/8/K7 b - - 0 1").unwrap();
        let only: Vec<Move> = pos.legal_moves().iter().copied().collect();
        assert_eq!(only.len(), 1);
        let params = EvalParams::material_only();
        let stop = AtomicBool::new(false);
        let tt = TranspositionTable::new(1);
        let mut ordering = OrderingTables::default();
        let mut searcher = Searcher::new(
            &params,
            SearchParams::default(),
            &tt,
            &mut ordering,
            Techniques::default(),
            StopConditions {
                stop: &stop,
                deadline: None,
                max_nodes: None,
            },
            &[],
        );
        // As a singular extension's search would: the node at ply 1, its hash move left out.
        searcher.excluded[1] = Some(only[0]);
        let mut pv = Vec::new();
        let (alpha, beta) = (1000, 1001);
        assert_eq!(searcher.negamax(&pos, 4, 1, alpha, beta, &mut pv), alpha);
        assert!(tt.probe(pos.hash()).is_none());
    }

    #[test]
    fn double_extensions_extend_singular_moves_within_the_limit() {
        // Depths 1 to 8, as iterative deepening searches them, so that the table has entries
        // for the singular test; the counts at the end.
        let counts = |techniques: Techniques, search_params: SearchParams| {
            let pos = Position::from_fen(
                "r2q1rk1/pp2bppp/2n1pn2/3p4/3P4/2NBPN2/PP3PPP/R2Q1RK1 w - - 0 10",
            )
            .unwrap();
            let params = EvalParams::material_only();
            let stop = AtomicBool::new(false);
            let tt = TranspositionTable::new(1);
            let mut ordering = OrderingTables::default();
            let mut searcher = Searcher::new(
                &params,
                search_params,
                &tt,
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
            for depth in 1..=8 {
                searcher.search_depth(&pos, &mut moves, depth, None);
            }
            // Every double extension counted on the way down is taken off on the way up.
            assert_eq!(searcher.double_extensions, 0);
            searcher.stats()
        };
        let with = Techniques {
            double_extension: true,
            ..Techniques::default()
        };
        let on = counts(with, SearchParams::default());
        assert!(on.double_extensions > 0);
        assert!(on.double_extensions <= on.singular_extensions);
        // Off by default.
        let off = counts(Techniques::default(), SearchParams::default());
        assert_eq!(off.double_extensions, 0);
        let no_room = SearchParams {
            double_extension_limit: 0,
            ..SearchParams::default()
        };
        assert_eq!(counts(with, no_room).double_extensions, 0);
    }
}
