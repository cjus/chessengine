//! Training examples: a record turned into what the network trains on.
//!
//! The network sees the board from the side to move's view first, so an example lists the
//! inputs that are on from that view and then from the other side's (see
//! `cb2026_nnue::features`). The trainer never builds a `Position`: a record's pieces are all
//! it needs.
//!
//! ## The target
//!
//! The network's output `y` means `y × eval scale` centipawns, and `σ(y)` the expected result
//! for the side to move (`σ(x) = 1 / (1 + e^−x)`: 0 for a loss, ½ for a draw, 1 for a win). The
//! target is what `σ(y)` should be, a blend of two estimates of the same thing:
//!
//! ```text
//! target = (1 − w) × σ(score / eval scale) + w × result
//! ```
//!
//! The search's score is precise but sometimes wrong; the game's result is right about the game
//! but noisy about any one position in it. `w`, the **result weight**, sets the mix. Both are
//! from the side to move's view: records keep White's, so for Black the score changes sign and
//! the result becomes 1 − result.

use cb2026_chess::Color;
use cb2026_nnue::{Record, features};

/// The most inputs that are on in a position: one per piece.
pub const MAX_ACTIVE: usize = 32;

/// How a record's score and result become a target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Targets {
    /// Centipawns per unit of the network's output (see [`crate::scale`]).
    pub eval_scale: f32,
    /// How much of the target is the game's result rather than the search's score, 0 to 1.
    pub result_weight: f32,
}

/// One position, ready for the network.
#[derive(Clone, Debug, PartialEq)]
pub struct Example {
    /// The inputs that are on: `[0]` from the side to move's view, `[1]` from the other side's.
    inputs: [[u16; MAX_ACTIVE]; 2],
    /// How many inputs are on, the same from both views: one per piece.
    count: usize,
    /// What `σ(output)` should be, from the side to move's view.
    pub target: f32,
}

impl Example {
    // ANCHOR: example
    /// The example for `record`.
    pub fn from_record(record: &Record, targets: Targets) -> Example {
        let us = record.side_to_move();
        let mut inputs = [[0u16; MAX_ACTIVE]; 2];
        let mut count = 0;
        for (piece, square) in record.pieces() {
            inputs[0][count] = features::index(us, piece, square) as u16;
            inputs[1][count] = features::index(!us, piece, square) as u16;
            count += 1;
        }
        let score = f32::from(record.score_for_side_to_move());
        let result = match us {
            Color::White => record.result().for_white(),
            Color::Black => 1.0 - record.result().for_white(),
        };
        let w = targets.result_weight;
        let target = (1.0 - w) * sigmoid(score / targets.eval_scale) + w * result;
        Example {
            inputs,
            count,
            target,
        }
    }
    // ANCHOR_END: example

    /// The inputs that are on from one view: 0 for the side to move's, 1 for the other side's.
    pub fn inputs(&self, view: usize) -> &[u16] {
        &self.inputs[view][..self.count]
    }
}

/// The logistic function, `1 / (1 + e^−x)`: turns a score into an expected result.
pub fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use cb2026_chess::{Position, STARTING_FEN};
    use cb2026_nnue::GameResult;

    use super::*;

    const TARGETS: Targets = Targets {
        eval_scale: 146.0,
        result_weight: 0.25,
    };

    #[test]
    fn the_target_blends_score_and_result_from_the_side_to_moves_view() {
        let mut pos = Position::from_fen(STARTING_FEN).unwrap();
        let mv = pos.legal_moves()[0];
        // White to move, +146 cp for White (σ(1) ≈ 0.731), White won.
        let white = Record::new(&pos, 146, GameResult::WhiteWin, mv).unwrap();
        let expected = 0.75 * sigmoid(1.0) + 0.25 * 1.0;
        assert!((Example::from_record(&white, TARGETS).target - expected).abs() < 1e-6);
        // Black to move with the same White-side numbers: Black is behind and lost.
        pos.make_move(mv);
        let reply = pos.legal_moves()[0];
        let black = Record::new(&pos, 146, GameResult::WhiteWin, reply).unwrap();
        let expected = 0.75 * sigmoid(-1.0) + 0.25 * 0.0;
        assert!((Example::from_record(&black, TARGETS).target - expected).abs() < 1e-6);
    }

    #[test]
    fn the_inputs_are_the_side_to_moves_view_first() {
        let mut pos = Position::from_fen(STARTING_FEN).unwrap();
        pos.make_move(pos.legal_moves()[0]);
        let record = Record::new(&pos, 0, GameResult::Draw, pos.legal_moves()[0]).unwrap();
        let example = Example::from_record(&record, TARGETS);
        let sorted = |inputs: &[u16]| {
            let mut v: Vec<usize> = inputs.iter().map(|&i| usize::from(i)).collect();
            v.sort_unstable();
            v
        };
        let mut black: Vec<usize> = features::active(&pos, Color::Black).collect();
        let mut white: Vec<usize> = features::active(&pos, Color::White).collect();
        black.sort_unstable();
        white.sort_unstable();
        assert_eq!(sorted(example.inputs(0)), black);
        assert_eq!(sorted(example.inputs(1)), white);
    }
}
