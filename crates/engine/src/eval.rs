//! Evaluation: how good a position looks, without searching.
//!
//! The search can't look all the way to the end of the game, so at the edge of its tree it
//! asks the evaluation for a number: positive if the position favours the side to move,
//! negative if it favours the opponent, in **centipawns** (hundredths of a pawn).
//!
//! ## Material (stage 5)
//!
//! The simplest useful evaluation counts material. It wins material and finds mates, but it
//! has no idea where pieces belong: to it, a knight on a1 is as good as a knight on e5.
//!
//! ## Piece-square tables (stage 9)
//!
//! A **piece-square table** (PST) gives each piece type a bonus or penalty for every square.
//! Knights are worth more in the centre, where they reach eight squares, than in a corner,
//! where they reach two. Pawns gain value as they advance. The king wants shelter behind its
//! pawns while queens are on the board, and the centre once they're gone. A PST encodes that
//! kind of knowledge in a table lookup, which costs almost nothing.
//!
//! ## Tapered evaluation
//!
//! What's good in the middlegame is often bad in the endgame (the king's best square, most of
//! all). So every value comes in two versions, **middlegame** (mg) and **endgame** (eg), and
//! the evaluation blends them according to the material left on the board, the **phase**:
//!
//! ```text
//! phase = knights + bishops + 2 × rooks + 4 × queens     (24 at the start, capped at 24)
//! score = (mg × phase + eg × (24 − phase)) / 24
//! ```
//!
//! Blending, instead of switching at some point, avoids a jump in the score when one piece is
//! traded, which the search would otherwise try to exploit.
//!
//! ## The full evaluation (stage 15)
//!
//! Mobility, pawn structure, passed pawns, the bishop pair, rook files, king safety and tempo
//! live in [`crate::hce`]. [`trace`] collects every term for each side.
//!
//! The tables below are cb2026's own starting values, written by hand from the principles
//! above. They are rough on purpose: stage 16 tunes every number on self-play games.
//!
//! See the Chess Programming Wiki, [Evaluation](https://www.chessprogramming.org/Evaluation),
//! [Piece-Square Tables](https://www.chessprogramming.org/Piece-Square_Tables) and
//! [Tapered Eval](https://www.chessprogramming.org/Tapered_Eval).

use cb2026_chess::{Color, PieceType, Position, Square};

use crate::hce::{self, TermParams};

/// The phase of the starting position: the most material the phase counts.
pub const MAX_PHASE: i32 = 24;

/// A piece-square table: one value per square, written **from White's side with rank 8 at
/// the top**, the way a board is printed. Index it with [`pst_index`].
pub type Pst = [i32; 64];

// ANCHOR: params
/// The evaluation's tunable numbers, and switches for its term groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalParams {
    /// Middlegame value of each piece type in centipawns, indexed by [`PieceType::index`]
    /// (the king's value is never used: both sides always have one).
    pub mg_values: [i32; 6],
    /// Endgame value of each piece type.
    pub eg_values: [i32; 6],
    /// Middlegame piece-square tables, indexed by [`PieceType::index`].
    pub mg_pst: [Pst; 6],
    /// Endgame piece-square tables.
    pub eg_pst: [Pst; 6],
    /// How much each piece type counts toward the game phase.
    pub phase_weights: [i32; 6],
    /// Use the piece-square tables (off: material only).
    pub use_pst: bool,
    /// Blend middlegame and endgame values by phase (off: middlegame values only).
    pub tapered: bool,
    /// The positional terms of the full evaluation (stage 15).
    pub terms: TermParams,
}
// ANCHOR_END: params

impl Default for EvalParams {
    /// The hand-set values. The tuned ones ([`EvalParams::tuned`]) didn't measure stronger
    /// (see book chapter 16), so they aren't the default.
    fn default() -> EvalParams {
        EvalParams::hand_set()
    }
}

impl EvalParams {
    /// Copies every tunable value from `other`, keeping this one's switches.
    pub fn copy_values(&mut self, other: &EvalParams) {
        for param in Param::all() {
            self.set_value(param, other.value(param));
        }
    }

    /// The values found by Texel tuning (stage 16, see [`crate::tuned`]).
    pub fn tuned() -> EvalParams {
        let mut params = EvalParams::hand_set();
        for param in Param::all() {
            params.set_value(param, crate::tuned::TUNED[param.index()]);
        }
        params
    }

    /// The hand-written starting values from stages 9 and 15, before tuning.
    pub fn hand_set() -> EvalParams {
        EvalParams {
            mg_values: [90, 320, 330, 480, 960, 0],
            eg_values: [120, 300, 320, 530, 940, 0],
            mg_pst: [PAWN_MG, KNIGHT, BISHOP, ROOK_MG, QUEEN_MG, KING_MG],
            eg_pst: [PAWN_EG, KNIGHT, BISHOP, ROOK_EG, QUEEN_EG, KING_EG],
            phase_weights: [0, 1, 1, 2, 4, 0],
            use_pst: true,
            tapered: true,
            terms: TermParams::default(),
        }
    }

    /// The stage 5 evaluation: material only, the same in every phase (P 100, N 320, B 330,
    /// R 500, Q 900). Scores are easy to work out by hand, which makes it useful in tests.
    pub fn material_only() -> EvalParams {
        let values = [100, 320, 330, 500, 900, 0];
        EvalParams {
            mg_values: values,
            eg_values: values,
            use_pst: false,
            tapered: false,
            terms: TermParams::none(),
            ..EvalParams::default()
        }
    }
}

/// The table index for a piece of `color` on `square`. Tables are written from White's side
/// with rank 8 first, so White's squares are flipped vertically and Black's are used as they
/// are (a black piece on a8 is on its own "a1").
// ANCHOR: pst_index
pub fn pst_index(color: Color, square: Square) -> usize {
    match color {
        Color::White => square.index() ^ 56,
        Color::Black => square.index(),
    }
}
// ANCHOR_END: pst_index

/// A middlegame and an endgame value, in centipawns.
pub type MgEg = (i32, i32);

/// An evaluation split into its parts, for the shell's `eval` command. Each term holds one
/// value per side (indexed by [`Color::index`]), each from that side's own point of view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvalTrace {
    /// Material.
    pub material: [MgEg; 2],
    /// Piece-square tables.
    pub pst: [MgEg; 2],
    /// Mobility.
    pub mobility: [MgEg; 2],
    /// Pawn structure: doubled and isolated pawns.
    pub pawns: [MgEg; 2],
    /// Passed pawns.
    pub passed: [MgEg; 2],
    /// Bishop pair and rook files.
    pub pieces: [MgEg; 2],
    /// King safety: pawn shield and attacks on the king zone.
    pub king_safety: [MgEg; 2],
    /// The side to move's tempo bonus.
    pub tempo: [MgEg; 2],
    /// The game phase, from 0 (bare kings and pawns) to [`MAX_PHASE`].
    pub phase: i32,
    /// The blended total, from White's point of view.
    pub total: i32,
}

impl EvalTrace {
    /// Every term with its name, in display order.
    pub fn terms(&self) -> [(&'static str, [MgEg; 2]); 8] {
        [
            ("Material", self.material),
            ("Piece-squares", self.pst),
            ("Mobility", self.mobility),
            ("Pawn structure", self.pawns),
            ("Passed pawns", self.passed),
            ("Pieces", self.pieces),
            ("King safety", self.king_safety),
            ("Tempo", self.tempo),
        ]
    }

    /// White's total minus Black's, middlegame and endgame.
    pub fn net(&self) -> MgEg {
        self.terms().iter().fold((0, 0), |(mg, eg), (_, [w, b])| {
            (mg + w.0 - b.0, eg + w.1 - b.1)
        })
    }
}

// ANCHOR: param
/// One evaluation parameter: a `(middlegame, endgame)` pair in [`EvalParams`]. Every term
/// of the evaluation is "this parameter, this many times, for this side".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Param {
    /// A piece's value.
    Material(PieceType),
    /// A piece-square table entry (table index, see [`pst_index`]).
    Pst(PieceType, usize),
    /// Mobility per safe square.
    Mobility(PieceType),
    /// A doubled pawn.
    Doubled,
    /// An isolated pawn.
    Isolated,
    /// A passed pawn on this relative rank.
    Passed(usize),
    /// The bishop pair.
    BishopPair,
    /// A rook on an open file.
    RookOpenFile,
    /// A rook on a semi-open file.
    RookSemiOpenFile,
    /// A pawn in the king's shield.
    PawnShield,
    /// The side to move.
    Tempo,
}

impl Param {
    /// How many parameters there are.
    pub const COUNT: usize = 411;

    /// The parameter's position in a flat list of all parameters, `0..COUNT`.
    pub fn index(self) -> usize {
        match self {
            Param::Material(pt) => pt.index(),
            Param::Pst(pt, square) => 6 + pt.index() * 64 + square,
            Param::Mobility(pt) => 390 + pt.index(),
            Param::Doubled => 396,
            Param::Isolated => 397,
            Param::Passed(rank) => 398 + rank,
            Param::BishopPair => 406,
            Param::RookOpenFile => 407,
            Param::RookSemiOpenFile => 408,
            Param::PawnShield => 409,
            Param::Tempo => 410,
        }
    }

    /// Every parameter, in [`Param::index`] order.
    pub fn all() -> Vec<Param> {
        let mut all = Vec::with_capacity(Param::COUNT);
        all.extend(PieceType::ALL.map(Param::Material));
        for pt in PieceType::ALL {
            all.extend((0..64).map(|square| Param::Pst(pt, square)));
        }
        all.extend(PieceType::ALL.map(Param::Mobility));
        all.extend([Param::Doubled, Param::Isolated]);
        all.extend((0..8).map(Param::Passed));
        all.extend([
            Param::BishopPair,
            Param::RookOpenFile,
            Param::RookSemiOpenFile,
            Param::PawnShield,
            Param::Tempo,
        ]);
        all
    }
}

impl EvalParams {
    /// The current value of `param`.
    pub fn value(&self, param: Param) -> MgEg {
        let t = &self.terms;
        match param {
            Param::Material(pt) => (self.mg_values[pt.index()], self.eg_values[pt.index()]),
            Param::Pst(pt, sq) => (self.mg_pst[pt.index()][sq], self.eg_pst[pt.index()][sq]),
            Param::Mobility(pt) => t.mobility[pt.index()],
            Param::Doubled => t.doubled,
            Param::Isolated => t.isolated,
            Param::Passed(rank) => t.passed[rank],
            Param::BishopPair => t.bishop_pair,
            Param::RookOpenFile => t.rook_open_file,
            Param::RookSemiOpenFile => t.rook_semi_open_file,
            Param::PawnShield => t.pawn_shield,
            Param::Tempo => t.tempo,
        }
    }

    /// Sets `param` to `value`.
    pub fn set_value(&mut self, param: Param, value: MgEg) {
        let t = &mut self.terms;
        match param {
            Param::Material(pt) => {
                self.mg_values[pt.index()] = value.0;
                self.eg_values[pt.index()] = value.1;
            }
            Param::Pst(pt, sq) => {
                self.mg_pst[pt.index()][sq] = value.0;
                self.eg_pst[pt.index()][sq] = value.1;
            }
            Param::Mobility(pt) => t.mobility[pt.index()] = value,
            Param::Doubled => t.doubled = value,
            Param::Isolated => t.isolated = value,
            Param::Passed(rank) => t.passed[rank] = value,
            Param::BishopPair => t.bishop_pair = value,
            Param::RookOpenFile => t.rook_open_file = value,
            Param::RookSemiOpenFile => t.rook_semi_open_file = value,
            Param::PawnShield => t.pawn_shield = value,
            Param::Tempo => t.tempo = value,
        }
    }
}

/// Receives the evaluation's terms as [`collect`] finds them. The evaluation itself adds up
/// their values ([`trace`]); the tuner records how often each parameter applies.
pub trait TermSink {
    /// `side` gets parameter `param`, `count` times (`count` may be negative).
    fn term(&mut self, side: Color, param: Param, count: i32);
    /// `side`'s king is in danger: a middlegame penalty that isn't a parameter times a count
    /// (it's quadratic in the attacks), so it is passed on as a value.
    fn king_danger(&mut self, side: Color, penalty: i32);
}
// ANCHOR_END: param

// ANCHOR: evaluate
/// Reports every term of the evaluation, for both sides, to `sink`.
pub fn collect<S: TermSink>(pos: &Position, params: &EvalParams, sink: &mut S) {
    for color in Color::ALL {
        for piece_type in PieceType::ALL {
            for square in pos.pieces(color, piece_type) {
                sink.term(color, Param::Material(piece_type), 1);
                if params.use_pst {
                    sink.term(color, Param::Pst(piece_type, pst_index(color, square)), 1);
                }
            }
        }
    }
    hce::collect(pos, &params.terms, sink);
}

/// The game phase: from 0 (bare kings and pawns) to [`MAX_PHASE`] (all the pieces).
pub fn phase(pos: &Position, params: &EvalParams) -> i32 {
    let phase: i32 = PieceType::ALL
        .iter()
        .map(|&pt| params.phase_weights[pt.index()] * pos.pieces_of_type(pt).count() as i32)
        .sum();
    phase.min(MAX_PHASE)
}

/// Adds up the terms' values into an [`EvalTrace`].
struct TraceSink<'a> {
    params: &'a EvalParams,
    trace: EvalTrace,
}

impl TermSink for TraceSink<'_> {
    fn term(&mut self, side: Color, param: Param, count: i32) {
        let t = &mut self.trace;
        let slot = match param {
            Param::Material(_) => &mut t.material,
            Param::Pst(..) => &mut t.pst,
            Param::Mobility(_) => &mut t.mobility,
            Param::Doubled | Param::Isolated => &mut t.pawns,
            Param::Passed(_) => &mut t.passed,
            Param::BishopPair | Param::RookOpenFile | Param::RookSemiOpenFile => &mut t.pieces,
            Param::PawnShield => &mut t.king_safety,
            Param::Tempo => &mut t.tempo,
        };
        let (mg, eg) = self.params.value(param);
        slot[side.index()].0 += mg * count;
        slot[side.index()].1 += eg * count;
    }

    fn king_danger(&mut self, side: Color, penalty: i32) {
        self.trace.king_safety[side.index()].0 -= penalty;
    }
}

/// Works out the evaluation's parts; the total is from White's point of view.
pub fn trace(pos: &Position, params: &EvalParams) -> EvalTrace {
    let mut sink = TraceSink {
        params,
        trace: EvalTrace::default(),
    };
    collect(pos, params, &mut sink);
    let mut t = sink.trace;
    t.phase = phase(pos, params);
    let (mg, eg) = t.net();
    t.total = if params.tapered {
        (mg * t.phase + eg * (MAX_PHASE - t.phase)) / MAX_PHASE
    } else {
        mg
    };
    t
}

/// The position's value in centipawns, from the side to move's point of view.
pub fn evaluate(pos: &Position, params: &EvalParams) -> i32 {
    let total = trace(pos, params).total;
    match pos.side_to_move() {
        Color::White => total,
        Color::Black => -total,
    }
}
// ANCHOR_END: evaluate

// ANCHOR: tables
// The tables, from White's side, rank 8 at the top.

#[rustfmt::skip]
const PAWN_MG: Pst = [
      0,   0,   0,   0,   0,   0,   0,   0,
     40,  40,  40,  40,  40,  40,  40,  40,
     10,  10,  15,  25,  25,  15,  10,  10,
      0,   5,  10,  20,  20,  10,   5,   0,
     -5,   0,   5,  15,  15,   5,   0,  -5,
     -5,   0,   0,   5,   5,  -5,   0,  -5,
      0,   5,   5, -15, -15,  10,  10,   0,
      0,   0,   0,   0,   0,   0,   0,   0,
];

#[rustfmt::skip]
const PAWN_EG: Pst = [
      0,   0,   0,   0,   0,   0,   0,   0,
     80,  80,  80,  80,  80,  80,  80,  80,
     45,  45,  45,  45,  45,  45,  45,  45,
     25,  25,  25,  25,  25,  25,  25,  25,
     10,  10,  10,  10,  10,  10,  10,  10,
      0,   0,   0,   0,   0,   0,   0,   0,
     -5,  -5,  -5,  -5,  -5,  -5,  -5,  -5,
      0,   0,   0,   0,   0,   0,   0,   0,
];

#[rustfmt::skip]
const KNIGHT: Pst = [
    -50, -35, -25, -20, -20, -25, -35, -50,
    -35, -15,   0,   5,   5,   0, -15, -35,
    -25,   5,  15,  20,  20,  15,   5, -25,
    -20,   5,  20,  25,  25,  20,   5, -20,
    -20,   0,  15,  25,  25,  15,   0, -20,
    -25,   5,  10,  15,  15,  10,   5, -25,
    -35, -15,   0,   5,   5,   0, -15, -35,
    -50, -30, -25, -20, -20, -25, -30, -50,
];

#[rustfmt::skip]
const BISHOP: Pst = [
    -20, -10, -10, -10, -10, -10, -10, -20,
    -10,   0,   0,   0,   0,   0,   0, -10,
    -10,   0,   5,  10,  10,   5,   0, -10,
    -10,   5,   5,  10,  10,   5,   5, -10,
    -10,   0,  10,  10,  10,  10,   0, -10,
    -10,  10,  10,  10,  10,  10,  10, -10,
    -10,  10,   0,   0,   0,   0,  10, -10,
    -20, -10, -15, -10, -10, -15, -10, -20,
];

#[rustfmt::skip]
const ROOK_MG: Pst = [
      0,   0,   0,   5,   5,   0,   0,   0,
     15,  20,  20,  20,  20,  20,  20,  15,
     -5,   0,   0,   0,   0,   0,   0,  -5,
     -5,   0,   0,   0,   0,   0,   0,  -5,
     -5,   0,   0,   0,   0,   0,   0,  -5,
     -5,   0,   0,   0,   0,   0,   0,  -5,
     -5,   0,   0,   0,   0,   0,   0,  -5,
     -5,   0,   0,   5,   5,   5,   0,  -5,
];

#[rustfmt::skip]
const ROOK_EG: Pst = [
      5,   5,   5,   5,   5,   5,   5,   5,
     10,  10,  10,  10,  10,  10,  10,  10,
      0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,
];

#[rustfmt::skip]
const QUEEN_MG: Pst = [
    -20, -10, -10,  -5,  -5, -10, -10, -20,
    -10,   0,   0,   0,   0,   0,   0, -10,
    -10,   0,   5,   5,   5,   5,   0, -10,
     -5,   0,   5,   5,   5,   5,   0,  -5,
     -5,   0,   5,   5,   5,   5,   0,  -5,
    -10,   0,   5,   5,   5,   5,   0, -10,
    -10,   0,   0,   0,   0,   0,   0, -10,
    -20, -10, -10,   0,  -5, -10, -10, -20,
];

#[rustfmt::skip]
const QUEEN_EG: Pst = [
    -20, -10, -10,  -5,  -5, -10, -10, -20,
    -10,   0,   5,   5,   5,   5,   0, -10,
    -10,   5,  10,  10,  10,  10,   5, -10,
     -5,   5,  10,  15,  15,  10,   5,  -5,
     -5,   5,  10,  15,  15,  10,   5,  -5,
    -10,   5,  10,  10,  10,  10,   5, -10,
    -10,   0,   5,   5,   5,   5,   0, -10,
    -20, -10, -10,  -5,  -5, -10, -10, -20,
];

#[rustfmt::skip]
const KING_MG: Pst = [
    -50, -50, -50, -60, -60, -50, -50, -50,
    -50, -50, -50, -60, -60, -50, -50, -50,
    -40, -40, -45, -55, -55, -45, -40, -40,
    -35, -40, -40, -50, -50, -40, -40, -35,
    -25, -30, -30, -40, -40, -30, -30, -25,
    -15, -20, -20, -25, -25, -20, -20, -15,
     10,  10,  -5, -15, -15,  -5,  10,  10,
     20,  30,  10, -10,   0,  10,  30,  20,
];

#[rustfmt::skip]
const KING_EG: Pst = [
    -50, -35, -25, -20, -20, -25, -35, -50,
    -30, -10,   0,   5,   5,   0, -10, -30,
    -25,   0,  15,  20,  20,  15,   0, -25,
    -20,   5,  20,  30,  30,  20,   5, -20,
    -20,   5,  20,  30,  30,  20,   5, -20,
    -25,   0,  15,  20,  20,  15,   0, -25,
    -30, -15,   0,   0,   0,   0, -15, -30,
    -50, -35, -25, -20, -20, -25, -35, -50,
];
// ANCHOR_END: tables

#[cfg(test)]
mod tests {

    #[test]
    fn hand_set_values_are_the_default_and_tuned_ones_are_available() {
        assert_eq!(EvalParams::default(), EvalParams::hand_set());
        let tuned = EvalParams::tuned();
        assert_ne!(tuned, EvalParams::hand_set());
        // Copying values keeps the switches.
        let mut params = EvalParams::hand_set();
        params.use_pst = false;
        params.copy_values(&tuned);
        assert!(!params.use_pst);
        for param in Param::all() {
            assert_eq!(params.value(param), tuned.value(param));
        }
    }

    use super::*;

    fn eval(fen: &str) -> i32 {
        evaluate(&Position::from_fen(fen).unwrap(), &EvalParams::default())
    }

    /// The same position with the colours swapped and the board flipped.
    fn mirror(fen: &str) -> String {
        let fields: Vec<&str> = fen.split(' ').collect();
        let swap_case = |s: &str| -> String {
            s.chars()
                .map(|c| {
                    if c.is_ascii_uppercase() {
                        c.to_ascii_lowercase()
                    } else {
                        c.to_ascii_uppercase()
                    }
                })
                .collect()
        };
        let board: Vec<String> = fields[0].split('/').rev().map(swap_case).collect();
        let side = if fields[1] == "w" { "b" } else { "w" };
        let castling = if fields[2] == "-" {
            "-".to_string()
        } else {
            let mut c: Vec<char> = swap_case(fields[2]).chars().collect();
            c.sort_by_key(|c| (c.is_ascii_lowercase(), *c));
            c.into_iter().collect()
        };
        let ep = if fields[3] == "-" {
            "-".to_string()
        } else {
            let (file, rank) = fields[3].split_at(1);
            format!("{file}{}", if rank == "3" { "6" } else { "3" })
        };
        format!(
            "{} {side} {castling} {ep} {} {}",
            board.join("/"),
            fields[4],
            fields[5]
        )
    }

    #[test]
    fn start_position_is_balanced_apart_from_the_tempo() {
        let tempo = EvalParams::default().value(Param::Tempo).0;
        assert_eq!(eval(cb2026_chess::STARTING_FEN), tempo);
    }

    #[test]
    fn mirrored_positions_score_the_same_for_the_side_to_move() {
        for fen in crate::bench::BENCH_POSITIONS {
            let mirrored = mirror(fen);
            assert_eq!(eval(fen), eval(&mirrored), "{fen} vs {mirrored}");
        }
    }

    #[test]
    fn material_only_counts_material_for_the_side_to_move() {
        let params = EvalParams::material_only();
        // White is a knight up.
        let white = Position::from_fen("4k3/8/8/8/8/8/8/3NK3 w - - 0 1").unwrap();
        assert_eq!(evaluate(&white, &params), 320);
        let black = Position::from_fen("4k3/8/8/8/8/8/8/3NK3 b - - 0 1").unwrap();
        assert_eq!(evaluate(&black, &params), -320);
    }

    #[test]
    fn knights_prefer_the_centre() {
        let centre = eval("4k3/8/8/4N3/8/8/8/4K3 w - - 0 1");
        let corner = eval("4k3/8/8/8/8/8/8/N3K3 w - - 0 1");
        assert!(centre > corner, "{centre} vs {corner}");
        // The hand-set tables are explicit about it.
        let hand = EvalParams::hand_set();
        let at = |fen: &str| evaluate(&Position::from_fen(fen).unwrap(), &hand);
        assert!(at("4k3/8/8/4N3/8/8/8/4K3 w - - 0 1") > at("4k3/8/8/8/8/8/8/N3K3 w - - 0 1") + 50);
    }

    #[test]
    fn the_king_hides_in_the_middlegame_and_centralises_in_the_endgame() {
        // The same Italian Game position, with White's king castled or walked to e2.
        let castled = eval("r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1");
        let exposed = eval("r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPPKPPP/R1BQ3R w - - 0 1");
        assert!(castled > exposed + 30, "{castled} vs {exposed}");
        // With only kings left, the centre is better than the corner.
        let central = eval("8/8/4k3/8/8/4K3/8/8 w - - 0 1");
        let corner = eval("8/8/4k3/8/8/8/8/K7 w - - 0 1");
        assert!(central > corner + 30, "{central} vs {corner}");
    }

    #[test]
    fn phase_goes_from_opening_to_endgame() {
        let params = EvalParams::default();
        let start = trace(&Position::startpos(), &params);
        assert_eq!(start.phase, MAX_PHASE);
        let kings = trace(
            &Position::from_fen("8/8/4k3/8/8/4K3/8/8 w - - 0 1").unwrap(),
            &params,
        );
        assert_eq!(kings.phase, 0);
        // Rook and pawn endgame: phase 4.
        let rooks = trace(
            &Position::from_fen("3rk3/5ppp/8/8/8/8/5PPP/3RK3 w - - 0 1").unwrap(),
            &params,
        );
        assert_eq!(rooks.phase, 4);
    }

    #[test]
    fn switches_turn_terms_off() {
        let pos = Position::from_fen("4k3/8/8/4N3/8/8/8/4K3 w - - 0 1").unwrap();
        let params = EvalParams {
            use_pst: false,
            tapered: false,
            terms: TermParams::none(),
            ..EvalParams::hand_set()
        };
        // Material only, middlegame value.
        assert_eq!(evaluate(&pos, &params), 320);
        let t = trace(&pos, &params);
        assert_eq!(t.pst, [(0, 0); 2]);
    }
}
