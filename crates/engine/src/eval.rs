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
//! The tables below are cb2026's own starting values, written by hand from the principles
//! above. They are rough on purpose: stage 16 tunes every number on self-play games.
//!
//! See the Chess Programming Wiki, [Evaluation](https://www.chessprogramming.org/Evaluation),
//! [Piece-Square Tables](https://www.chessprogramming.org/Piece-Square_Tables) and
//! [Tapered Eval](https://www.chessprogramming.org/Tapered_Eval).

use cb2026_chess::{Color, PieceType, Position, Square};

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
}
// ANCHOR_END: params

impl Default for EvalParams {
    fn default() -> EvalParams {
        EvalParams {
            mg_values: [90, 320, 330, 480, 960, 0],
            eg_values: [120, 300, 320, 530, 940, 0],
            mg_pst: [PAWN_MG, KNIGHT, BISHOP, ROOK_MG, QUEEN_MG, KING_MG],
            eg_pst: [PAWN_EG, KNIGHT, BISHOP, ROOK_EG, QUEEN_EG, KING_EG],
            phase_weights: [0, 1, 1, 2, 4, 0],
            use_pst: true,
            tapered: true,
        }
    }
}

impl EvalParams {
    /// The stage 5 evaluation: material only, the same in every phase (P 100, N 320, B 330,
    /// R 500, Q 900). Scores are easy to work out by hand, which makes it useful in tests.
    pub fn material_only() -> EvalParams {
        let values = [100, 320, 330, 500, 900, 0];
        EvalParams {
            mg_values: values,
            eg_values: values,
            use_pst: false,
            tapered: false,
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

/// An evaluation split into its parts, for the shell's `eval` command. All values are from
/// White's point of view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvalTrace {
    /// Material, middlegame and endgame.
    pub material: (i32, i32),
    /// Piece-square tables, middlegame and endgame.
    pub pst: (i32, i32),
    /// The game phase, from 0 (bare kings and pawns) to [`MAX_PHASE`].
    pub phase: i32,
    /// The blended total, from White's point of view.
    pub total: i32,
}

// ANCHOR: evaluate
/// Works out the evaluation's parts, from White's point of view.
pub fn trace(pos: &Position, params: &EvalParams) -> EvalTrace {
    let mut t = EvalTrace::default();
    let mut phase = 0;
    for color in Color::ALL {
        let sign = if color == Color::White { 1 } else { -1 };
        for piece_type in PieceType::ALL {
            let i = piece_type.index();
            for square in pos.pieces(color, piece_type) {
                t.material.0 += sign * params.mg_values[i];
                t.material.1 += sign * params.eg_values[i];
                if params.use_pst {
                    let at = pst_index(color, square);
                    t.pst.0 += sign * params.mg_pst[i][at];
                    t.pst.1 += sign * params.eg_pst[i][at];
                }
                phase += params.phase_weights[i];
            }
        }
    }
    t.phase = phase.min(MAX_PHASE);
    let mg = t.material.0 + t.pst.0;
    let eg = t.material.1 + t.pst.1;
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
    fn start_position_is_balanced() {
        assert_eq!(eval(cb2026_chess::STARTING_FEN), 0);
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
        assert!(centre > corner + 50, "{centre} vs {corner}");
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
            ..EvalParams::default()
        };
        // Material only, middlegame value.
        assert_eq!(evaluate(&pos, &params), 320);
        let t = trace(&pos, &params);
        assert_eq!(t.pst, (0, 0));
    }
}
