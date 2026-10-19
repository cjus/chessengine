//! Correction history: learning where the static evaluation goes wrong.
//!
//! The search trusts the static evaluation in many places: reverse futility pruning, razoring,
//! null-move pruning and futility pruning decide on it, and the quiescence search stands pat
//! on it. A hand-crafted evaluation has systematic errors. In some pawn structures it is too
//! optimistic and in others too pessimistic, by much the same amount each time the structure
//! comes up. The search finds this out as it goes: a node's searched score is a better estimate
//! than its static evaluation, and the difference between them is the evaluation's error there.
//!
//! **Correction history** keeps a running average of that difference in a table indexed by the
//! side to move and the pawn structure. It adds the average to every later static evaluation of
//! a position with the same pawns and the same side to move. The table persists from one search
//! to the next, like the history tables, and a new game clears it. It gained +28.0 ± 13.0 Elo
//! after 1,308 games at 8+0.08 (cjus/chessengine#112).
//!
//! ## Which results it learns from
//!
//! - Only the main search's nodes, which have a depth: a quiescence node's score is mostly its
//!   own stand pat.
//! - Not in check, where there is no static evaluation; not in a singular extension's search,
//!   whose score leaves the best move out; not when the search was stopped.
//! - Not when the best move is a capture or promotion: the score then includes material the
//!   static evaluation is not meant to see.
//! - A fail high is a lower bound on the true score, so it shows that the evaluation was too low
//!   only if the score is above it; a fail low is an upper bound, so it shows that the
//!   evaluation was too high only if the score is below it. Other bounds say nothing and are
//!   skipped.
//! - Not mate scores, which are about the line, not the evaluation.
//!
//! ## The update
//!
//! An entry moves towards the newest difference by `min(depth + 1, correction_max_weight)` 256ths
//! of the way: a deep search's result counts for more than a shallow one's. Entries are kept in
//! 256ths of a centipawn, so that small steps aren't rounded away, and no entry goes beyond
//! `correction_limit` centipawns either way.
//!
//! The difference is always taken against the uncorrected evaluation. So an entry is an average
//! of how far the raw evaluation was off, whatever corrections the search below it used, and it
//! settles instead of building on itself.
//!
//! ## The pawn key
//!
//! There are far too many pawn structures to give each its own entry, so the two sides' pawn
//! bitboards are mixed into one number and its low bits pick the entry ([`pawn_key`]). The
//! mixing is the finaliser of SplitMix64 (Steele, Lea and Flood, 2014). Computing it from the
//! bitboards costs a few multiplications per node, much less than the evaluation it corrects,
//! and leaves the position as it is: no incrementally updated pawn hash is needed.
//!
//! ## Pitfalls
//!
//! - **Collisions.** 16,384 entries per side are shared by every pawn structure the search
//!   meets, and two structures with the same index share an average. A bigger table collides
//!   less but learns each entry more slowly.
//! - **Small corrections hide in the margins.** Reverse futility pruning, razoring and futility
//!   pruning have margins of 80 centipawns or more per ply, so a correction of 10 or 20 changes
//!   few of their decisions. Null-move pruning, which only needs the evaluation at beta, and the
//!   stand pat feel it more directly.
//! - **Mate range.** A corrected evaluation is kept clear of the mate scores, so a large
//!   correction of a large evaluation can't look like a mate.
//!
//! The idea is from Caissa (MIT licence, 2023), and Stockfish (GPL-3.0) and most strong engines
//! have adopted it; this code is cb2026's own. See the Chess Programming Wiki,
//! [Static Evaluation Correction History](https://www.chessprogramming.org/Static_Evaluation_Correction_History).

use cb2026_chess::{Color, PieceType, Position};

use crate::search::{MATE, MAX_PLY, SearchParams};
use crate::tt::Bound;

/// Entries per side: a power of two, so that the index is the pawn key's low bits.
pub const CORRECTION_ENTRIES: usize = 16_384;
/// Corrections are kept in 1/256 of a centipawn, so that small updates aren't rounded away.
const GRAIN: i32 = 256;
/// An update's weight is out of this much.
const WEIGHT_SCALE: i32 = 256;

// ANCHOR: table
/// How far the static evaluation tends to be off, by side to move and pawn structure.
#[derive(Clone, Debug)]
pub struct CorrectionHistory {
    /// `2 × CORRECTION_ENTRIES` corrections in 256ths of a centipawn: White to move's, then
    /// Black to move's.
    table: Vec<i32>,
}
// ANCHOR_END: table

impl Default for CorrectionHistory {
    fn default() -> CorrectionHistory {
        CorrectionHistory {
            table: vec![0; 2 * CORRECTION_ENTRIES],
        }
    }
}

impl CorrectionHistory {
    /// Forgets everything (a new game).
    pub fn clear(&mut self) {
        self.table.fill(0);
    }

    /// The correction for `pos`'s static evaluation, in centipawns.
    pub fn correction(&self, pos: &Position) -> i32 {
        self.table[index(pos)] / GRAIN
    }

    // ANCHOR: corrected
    /// `raw_eval`, the static evaluation of `pos`, corrected, and kept clear of the mate scores.
    pub fn corrected(&self, pos: &Position, raw_eval: i32) -> i32 {
        let highest = MATE - MAX_PLY as i32 - 1;
        (raw_eval + self.correction(pos)).clamp(-highest, highest)
    }
    // ANCHOR_END: corrected

    // ANCHOR: update
    /// Learns from a search of `pos` to `depth` that scored `score` as a `bound`, where the
    /// uncorrected static evaluation was `raw_eval`. The caller has already left out the nodes
    /// that can't say anything about the evaluation (see the module documentation); this leaves
    /// out mate scores and bounds on the wrong side of the evaluation.
    pub fn update(
        &mut self,
        pos: &Position,
        depth: u32,
        raw_eval: i32,
        score: i32,
        bound: Bound,
        params: &SearchParams,
    ) {
        if score.abs() >= MATE - MAX_PLY as i32 {
            return;
        }
        // A fail high shows the evaluation too low, a fail low shows it too high; nothing else.
        let informative = match bound {
            Bound::Exact => true,
            Bound::Lower => score > raw_eval,
            Bound::Upper => score < raw_eval,
        };
        if !informative {
            return;
        }
        let weight = (depth as i32 + 1).min(params.correction_max_weight);
        let limit = params.correction_limit * GRAIN;
        let target = ((score - raw_eval) * GRAIN).clamp(-limit, limit);
        let entry = &mut self.table[index(pos)];
        *entry = (*entry * (WEIGHT_SCALE - weight) + target * weight) / WEIGHT_SCALE;
    }
    // ANCHOR_END: update
}

/// The table index for `pos`: the side to move, then the pawn key's low bits.
fn index(pos: &Position) -> usize {
    let side = pos.side_to_move().index();
    side * CORRECTION_ENTRIES + (pawn_key(pos) as usize & (CORRECTION_ENTRIES - 1))
}

// ANCHOR: pawn_key
/// The pawn structure as one number: both sides' pawn bitboards, mixed so that every bit of
/// them affects every bit of the result. Black's are mixed first, so a white pawn and a black
/// pawn on the same square don't count as the same structure.
pub fn pawn_key(pos: &Position) -> u64 {
    let white = pos.pieces(Color::White, PieceType::Pawn).0;
    let black = pos.pieces(Color::Black, PieceType::Pawn).0;
    mix(white ^ mix(black))
}

/// SplitMix64's finaliser (Steele, Lea and Flood, 2014): a cheap, thorough bit mixer.
fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}
// ANCHOR_END: pawn_key

#[cfg(test)]
mod tests {
    use super::*;

    fn after(moves: &[&str]) -> Position {
        let mut pos = Position::startpos();
        for uci in moves {
            let mv = pos.parse_uci_move(uci).unwrap();
            pos.make_move(mv);
        }
        pos
    }

    #[test]
    fn the_pawn_key_sees_only_pawns() {
        let start = pawn_key(&Position::startpos());
        // A knight move leaves the pawns as they were; a pawn move doesn't.
        assert_eq!(pawn_key(&after(&["g1f3"])), start);
        assert_ne!(pawn_key(&after(&["e2e4"])), start);
        // The same squares with the colours swapped are a different structure.
        let white = Position::from_fen("4k3/8/8/8/8/8/P7/4K3 w - - 0 1").unwrap();
        let black = Position::from_fen("4k3/8/8/8/8/8/p7/4K3 w - - 0 1").unwrap();
        assert_ne!(pawn_key(&white), pawn_key(&black));
    }

    #[test]
    fn updates_move_the_correction_towards_the_error() {
        let params = SearchParams::default();
        let pos = Position::startpos();
        let mut history = CorrectionHistory::default();
        // The evaluation says 20 and deep searches keep finding 50: the correction nears 30.
        for _ in 0..200 {
            history.update(&pos, 20, 20, 50, Bound::Exact, &params);
        }
        let learnt = history.correction(&pos);
        assert!((28..=30).contains(&learnt), "{learnt}");
        assert_eq!(history.corrected(&pos, 20), 20 + learnt);
        // It is only for this side to move and these pawns.
        assert_eq!(history.correction(&after(&["e2e4"])), 0);
        assert_eq!(history.correction(&after(&["g1f3"])), 0);
        assert_eq!(history.correction(&after(&["g1f3", "g8f6"])), learnt);
    }

    #[test]
    fn uninformative_results_are_ignored() {
        let params = SearchParams::default();
        let pos = Position::startpos();
        let mut history = CorrectionHistory::default();
        // A fail high below the evaluation and a fail low above it say nothing about it.
        history.update(&pos, 10, 20, -100, Bound::Lower, &params);
        history.update(&pos, 10, 20, 100, Bound::Upper, &params);
        // Nor does a mate score.
        history.update(&pos, 10, 20, MATE - 5, Bound::Exact, &params);
        assert_eq!(history.correction(&pos), 0);
        // On the right side of the evaluation, bounds count.
        history.update(&pos, 10, 20, 100, Bound::Lower, &params);
        assert!(history.correction(&pos) > 0);
    }

    #[test]
    fn corrections_are_limited() {
        let params = SearchParams::default();
        let pos = Position::startpos();
        let mut history = CorrectionHistory::default();
        for _ in 0..1000 {
            history.update(&pos, 30, 0, 2000, Bound::Exact, &params);
        }
        assert!(history.correction(&pos) <= params.correction_limit);
        // And a corrected evaluation never reaches the mate scores.
        assert!(history.corrected(&pos, MATE) < MATE - MAX_PLY as i32);
        history.clear();
        assert_eq!(history.correction(&pos), 0);
    }
}
