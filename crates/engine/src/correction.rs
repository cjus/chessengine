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
//! ## More tables (M5)
//!
//! The pawns are one part of the position whose errors repeat. The pieces are another: a
//! knight stuck on the rim, a badly placed king, a bishop pair the evaluation over- or
//! underrates. Two more tables, indexed the same way by the side to move and a key, use each
//! side's pieces other than pawns (knights, bishops, rooks, queens and king) as the key:
//! White's in one table, Black's in the other ([`non_pawn_key`]). Every update teaches all
//! three tables the same difference. The correction is then the pawn table's, counting
//! `correction_pawn_share` 256ths (all of it, 256), plus each piece table's, counting
//! `correction_non_pawn_share` 256ths (an eighth, 32), and no more than `correction_limit`
//! either way. With
//! [`Techniques::correction_non_pawn`](crate::search::Techniques::correction_non_pawn) off, the
//! pawn table alone corrects, as before.
//!
//! A move changes one side's pieces at most (a capture changes the other side's too), so in a
//! search most children share a piece key with their parent, and the tables keep learning about
//! the positions that matter. Stockfish (GPL-3.0) keeps tables like these, one keyed by each
//! side's pieces other than pawns, among others; this code is cb2026's own.
//!
//! **The first version averaged the tables**: half the pawn table's correction and a quarter
//! of each piece table's (`correction_pawn_share` 128, `correction_non_pawn_share` 64). It took
//! 4.3% off the bench and measured weaker: −20.7 ± 12.3 Elo after 1,362 games at 8+0.08
//! (cjus/chessengine#119). The piece tables weren't silent: at lookups they corrected by as
//! much as the pawn table. So they replaced half of a correction that works with one as large
//! and worse. This version keeps the pawn table whole and adds a little of the piece tables on
//! top, so they can only add to it.
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
//! ## The keys
//!
//! There are far too many pawn structures to give each its own entry, so the two sides' pawn
//! bitboards are mixed into one number and its low bits pick the entry ([`pawn_key`]). The
//! mixing is the finaliser of SplitMix64 (Steele, Lea and Flood, 2014). Computing it from the
//! bitboards costs a few multiplications per node, much less than the evaluation it corrects,
//! and leaves the position as it is: no incrementally updated pawn hash is needed. The piece
//! keys are made the same way, from five bitboards each.
//!
//! ## Pitfalls
//!
//! - **Collisions.** 16,384 entries per side are shared by every pawn structure the search
//!   meets, and two structures with the same index share an average. A bigger table collides
//!   less but learns each entry more slowly. The piece keys have more values still, so their
//!   tables collide more.
//! - **Small corrections hide in the margins.** Reverse futility pruning, razoring and futility
//!   pruning have margins of 80 centipawns or more per ply, so a correction of 10 or 20 changes
//!   few of their decisions. Null-move pruning, which only needs the evaluation at beta, and the
//!   stand pat feel it more directly.
//! - **Averaging dilutes.** The first version counted the pawn table half as much as alone,
//!   which may be what lost it. Adding instead keeps the pawn table's correction, at the cost
//!   of correcting by more than any one table where they agree; the limit still holds.
//! - **Mate range.** A corrected evaluation is kept clear of the mate scores, so a large
//!   correction of a large evaluation can't look like a mate.
//!
//! The idea is from Caissa (MIT licence, 2023), and Stockfish (GPL-3.0) and most strong engines
//! have adopted it; this code is cb2026's own. See the Chess Programming Wiki,
//! [Static Evaluation Correction History](https://www.chessprogramming.org/Static_Evaluation_Correction_History).

use cb2026_chess::{Color, PieceType, Position};

use crate::search::{MATE, MAX_PLY, SearchParams};
use crate::tt::Bound;

/// Entries per side: a power of two, so that the index is the key's low bits.
pub const CORRECTION_ENTRIES: usize = 16_384;
/// Corrections are kept in 1/256 of a centipawn, so that small updates aren't rounded away.
const GRAIN: i32 = 256;
/// An update's weight is out of this much.
const WEIGHT_SCALE: i32 = 256;
/// The tables' shares of the correction are out of this much.
const SHARE_SCALE: i32 = 256;
/// The pieces in a side's non-pawn key.
const NON_PAWN_PIECES: [PieceType; 5] = [
    PieceType::Knight,
    PieceType::Bishop,
    PieceType::Rook,
    PieceType::Queen,
    PieceType::King,
];

/// How correction history learns and corrects: the search's parameters, and whether the
/// piece tables are switched on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorrectionSettings {
    /// An update moves an entry at most this many 256ths of the way (`correction_max_weight`).
    pub max_weight: i32,
    /// No entry, and no correction, goes beyond this many centipawns (`correction_limit`).
    pub limit: i32,
    /// With the piece tables on, the pawn table's share of the correction, in 256ths
    /// (`correction_pawn_share`).
    pub pawn_share: i32,
    /// Each piece table's share of the correction, in 256ths (`correction_non_pawn_share`): 0
    /// when the piece tables are off, and then they don't learn either.
    pub non_pawn_share: i32,
}

impl CorrectionSettings {
    /// The settings from `params`, with the piece tables if `non_pawn`.
    pub fn new(params: &SearchParams, non_pawn: bool) -> CorrectionSettings {
        CorrectionSettings {
            max_weight: params.correction_max_weight,
            limit: params.correction_limit,
            pawn_share: params.correction_pawn_share,
            non_pawn_share: if non_pawn {
                params.correction_non_pawn_share
            } else {
                0
            },
        }
    }
}

// ANCHOR: table
/// How far the static evaluation tends to be off, by side to move and pawn structure, and by
/// side to move and each side's other pieces.
#[derive(Clone, Debug)]
pub struct CorrectionHistory {
    /// `2 × CORRECTION_ENTRIES` corrections in 256ths of a centipawn, by pawn structure: White
    /// to move's, then Black to move's.
    pawn: Vec<i32>,
    /// The same, by White's pieces other than pawns, then by Black's.
    non_pawn: [Vec<i32>; 2],
}
// ANCHOR_END: table

impl Default for CorrectionHistory {
    fn default() -> CorrectionHistory {
        let table = || vec![0; 2 * CORRECTION_ENTRIES];
        CorrectionHistory {
            pawn: table(),
            non_pawn: [table(), table()],
        }
    }
}

impl CorrectionHistory {
    /// Forgets everything (a new game).
    pub fn clear(&mut self) {
        self.pawn.fill(0);
        for table in &mut self.non_pawn {
            table.fill(0);
        }
    }

    // ANCHOR: correction
    /// The correction for `pos`'s static evaluation, in centipawns. With the piece tables off
    /// it is the pawn table's; on, the pawn table's counts `pawn_share` 256ths and each side's
    /// piece table's `non_pawn_share` 256ths, added up and kept within the limit.
    pub fn correction(&self, pos: &Position, settings: &CorrectionSettings) -> i32 {
        let pawn = self.pawn[index(pos, pawn_key(pos))];
        let non_pawn_share = settings.non_pawn_share;
        if non_pawn_share == 0 {
            return pawn / GRAIN;
        }
        let pieces: i32 = Color::ALL
            .iter()
            .map(|&color| self.non_pawn[color.index()][index(pos, non_pawn_key(pos, color))])
            .sum();
        let sum = pawn * settings.pawn_share + pieces * non_pawn_share;
        (sum / (SHARE_SCALE * GRAIN)).clamp(-settings.limit, settings.limit)
    }
    // ANCHOR_END: correction

    // ANCHOR: corrected
    /// `raw_eval`, the static evaluation of `pos`, corrected (see [`Self::correction`]), and
    /// kept clear of the mate scores.
    pub fn corrected(&self, pos: &Position, raw_eval: i32, settings: &CorrectionSettings) -> i32 {
        let highest = MATE - MAX_PLY as i32 - 1;
        (raw_eval + self.correction(pos, settings)).clamp(-highest, highest)
    }
    // ANCHOR_END: corrected

    // ANCHOR: update
    /// Learns from a search of `pos` to `depth` that scored `score` as a `bound`, where the
    /// uncorrected static evaluation was `raw_eval`: the pawn table, and the piece tables too
    /// if they are on. The caller has already left out the nodes that can't say anything about
    /// the evaluation (see the module documentation); this leaves out mate scores and bounds on
    /// the wrong side of the evaluation.
    pub fn update(
        &mut self,
        pos: &Position,
        depth: u32,
        raw_eval: i32,
        score: i32,
        bound: Bound,
        settings: &CorrectionSettings,
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
        let weight = (depth as i32 + 1).min(settings.max_weight);
        let limit = settings.limit * GRAIN;
        let target = ((score - raw_eval) * GRAIN).clamp(-limit, limit);
        let learn = |entry: &mut i32| {
            *entry = (*entry * (WEIGHT_SCALE - weight) + target * weight) / WEIGHT_SCALE;
        };
        learn(&mut self.pawn[index(pos, pawn_key(pos))]);
        if settings.non_pawn_share > 0 {
            for color in Color::ALL {
                learn(&mut self.non_pawn[color.index()][index(pos, non_pawn_key(pos, color))]);
            }
        }
    }
    // ANCHOR_END: update
}

/// The table index for `pos` with `key`: the side to move, then the key's low bits.
fn index(pos: &Position, key: u64) -> usize {
    let side = pos.side_to_move().index();
    side * CORRECTION_ENTRIES + (key as usize & (CORRECTION_ENTRIES - 1))
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

// ANCHOR: non_pawn_key
/// `color`'s pieces other than pawns as one number, made like [`pawn_key`]: each piece type's
/// bitboard is mixed into the key in turn, so a knight and a bishop on the same square differ.
pub fn non_pawn_key(pos: &Position, color: Color) -> u64 {
    NON_PAWN_PIECES
        .iter()
        .fold(0, |key, &piece| mix(key ^ pos.pieces(color, piece).0))
}
// ANCHOR_END: non_pawn_key

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

    /// The default settings, with the pawn table alone and with the piece tables.
    fn pawn_only() -> CorrectionSettings {
        CorrectionSettings::new(&SearchParams::default(), false)
    }

    fn all_tables() -> CorrectionSettings {
        CorrectionSettings::new(&SearchParams::default(), true)
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
    fn a_piece_key_sees_only_its_sides_other_pieces() {
        let start = Position::startpos();
        let key = |pos: &Position, color| non_pawn_key(pos, color);
        // A pawn move leaves both piece keys as they were; a knight move changes White's only.
        let e4 = after(&["e2e4"]);
        assert_eq!(key(&e4, Color::White), key(&start, Color::White));
        let nf3 = after(&["g1f3"]);
        assert_ne!(key(&nf3, Color::White), key(&start, Color::White));
        assert_eq!(key(&nf3, Color::Black), key(&start, Color::Black));
        // A knight and a bishop on the same square are different pieces.
        let knight = Position::from_fen("4k3/8/8/8/8/8/8/2N1K3 w - - 0 1").unwrap();
        let bishop = Position::from_fen("4k3/8/8/8/8/8/8/2B1K3 w - - 0 1").unwrap();
        assert_ne!(key(&knight, Color::White), key(&bishop, Color::White));
    }

    #[test]
    fn updates_move_the_correction_towards_the_error() {
        let pos = Position::startpos();
        let mut history = CorrectionHistory::default();
        // The evaluation says 20 and deep searches keep finding 50: the correction nears 30.
        for _ in 0..200 {
            history.update(&pos, 20, 20, 50, Bound::Exact, &pawn_only());
        }
        let learnt = history.correction(&pos, &pawn_only());
        assert!((28..=30).contains(&learnt), "{learnt}");
        assert_eq!(history.corrected(&pos, 20, &pawn_only()), 20 + learnt);
        // It is only for this side to move and these pawns.
        assert_eq!(history.correction(&after(&["e2e4"]), &pawn_only()), 0);
        assert_eq!(history.correction(&after(&["g1f3"]), &pawn_only()), 0);
        assert_eq!(
            history.correction(&after(&["g1f3", "g8f6"]), &pawn_only()),
            learnt
        );
        // The piece tables weren't taught, so added in they change nothing.
        assert_eq!(history.correction(&pos, &all_tables()), learnt);
    }

    #[test]
    fn the_piece_tables_learn_and_add_to_the_pawn_table() {
        let pos = Position::startpos();
        let mut history = CorrectionHistory::default();
        for _ in 0..200 {
            history.update(&pos, 20, 20, 50, Bound::Exact, &all_tables());
        }
        // All three tables learnt the same error: the pawn table's whole, plus an eighth of
        // each piece table's, makes a quarter more.
        let alone = history.correction(&pos, &pawn_only());
        assert!((28..=30).contains(&alone), "{alone}");
        let added = history.correction(&pos, &all_tables());
        assert!(
            (alone + alone / 4..=alone + alone / 4 + 1).contains(&added),
            "{added}"
        );
        // After 1. Nf3 Nf6 the pawns are as they were, but both sides' pieces have moved: the
        // pawn table's correction is left, whole.
        let knights_out = after(&["g1f3", "g8f6"]);
        assert_eq!(history.correction(&knights_out, &pawn_only()), alone);
        assert_eq!(history.correction(&knights_out, &all_tables()), alone);
        // After 1. e4 e5 only the pawns have changed: only the piece tables' eighths are left.
        let e4_e5 = after(&["e2e4", "e7e5"]);
        assert_eq!(history.correction(&e4_e5, &pawn_only()), 0);
        assert_eq!(history.correction(&e4_e5, &all_tables()), alone / 4);
        history.clear();
        assert_eq!(history.correction(&pos, &all_tables()), 0);
    }

    #[test]
    fn uninformative_results_are_ignored() {
        let pos = Position::startpos();
        let mut history = CorrectionHistory::default();
        // A fail high below the evaluation and a fail low above it say nothing about it.
        history.update(&pos, 10, 20, -100, Bound::Lower, &all_tables());
        history.update(&pos, 10, 20, 100, Bound::Upper, &all_tables());
        // Nor does a mate score.
        history.update(&pos, 10, 20, MATE - 5, Bound::Exact, &all_tables());
        assert_eq!(history.correction(&pos, &all_tables()), 0);
        // On the right side of the evaluation, bounds count.
        history.update(&pos, 10, 20, 100, Bound::Lower, &all_tables());
        assert!(history.correction(&pos, &all_tables()) > 0);
    }

    #[test]
    fn corrections_are_limited() {
        let pos = Position::startpos();
        let mut history = CorrectionHistory::default();
        for _ in 0..1000 {
            history.update(&pos, 30, 0, 2000, Bound::Exact, &all_tables());
        }
        assert!(history.correction(&pos, &pawn_only()) <= all_tables().limit);
        assert!(history.correction(&pos, &all_tables()) <= all_tables().limit);
        // And a corrected evaluation never reaches the mate scores.
        assert!(history.corrected(&pos, MATE, &all_tables()) < MATE - MAX_PLY as i32);
        history.clear();
        assert_eq!(history.correction(&pos, &pawn_only()), 0);
    }
}
