//! Evaluation: how good a position looks, without searching.
//!
//! The search can't look all the way to the end of the game, so at the edge of its tree it
//! asks the evaluation for a number: positive if the position favours the side to move,
//! negative if it favours the opponent, in **centipawns** (hundredths of a pawn).
//!
//! ## Stage 5: material only
//!
//! The simplest useful evaluation counts material with the classic piece values:
//!
//! | Pawn | Knight | Bishop | Rook | Queen |
//! | ---: | -----: | -----: | ---: | ----: |
//! | 100  | 320    | 330    | 500  | 900   |
//!
//! It knows nothing about where pieces stand, pawn structure or king safety. Even so, with a
//! search behind it, it wins material, avoids losing it, and finds mates. Stage 9 adds
//! piece-square tables; stages 15–16 build and tune a full evaluation.
//!
//! See the Chess Programming Wiki, [Evaluation](https://www.chessprogramming.org/Evaluation)
//! and [Point Value](https://www.chessprogramming.org/Point_Value).

use cb2026_chess::{Color, PieceType, Position};

// ANCHOR: params
/// The evaluation's tunable numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalParams {
    /// Value of each piece type in centipawns, indexed by [`PieceType::index`] (the king's
    /// value is never used: both sides always have one).
    pub piece_values: [i32; 6],
}

impl Default for EvalParams {
    fn default() -> EvalParams {
        EvalParams {
            piece_values: [100, 320, 330, 500, 900, 0],
        }
    }
}
// ANCHOR_END: params

// ANCHOR: evaluate
/// The position's value in centipawns, from the side to move's point of view.
pub fn evaluate(pos: &Position, params: &EvalParams) -> i32 {
    let material = |color: Color| -> i32 {
        PieceType::ALL
            .iter()
            .map(|&pt| pos.pieces(color, pt).count() as i32 * params.piece_values[pt.index()])
            .sum()
    };
    let us = pos.side_to_move();
    material(us) - material(!us)
}
// ANCHOR_END: evaluate

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_position_is_balanced() {
        assert_eq!(evaluate(&Position::startpos(), &EvalParams::default()), 0);
    }

    #[test]
    fn counts_material_for_the_side_to_move() {
        // White is a knight up.
        let white = Position::from_fen("4k3/8/8/8/8/8/8/3NK3 w - - 0 1").unwrap();
        assert_eq!(evaluate(&white, &EvalParams::default()), 320);
        let black = Position::from_fen("4k3/8/8/8/8/8/8/3NK3 b - - 0 1").unwrap();
        assert_eq!(evaluate(&black, &EvalParams::default()), -320);
    }
}
