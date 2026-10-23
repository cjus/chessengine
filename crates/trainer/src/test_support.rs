//! Helpers for this crate's tests: random networks and records from random games.

use cb2026_chess::{Position, STARTING_FEN};
use cb2026_nnue::{Activation, FloatNetwork, GameResult, Record};

use crate::train::Rng;

/// A float network with random weights of a realistic size.
pub fn random_network(hidden: usize, activation: Activation, seed: u64) -> FloatNetwork {
    let mut rng = Rng::new(seed);
    let mut net = FloatNetwork::zeros(hidden, activation, 146);
    net.input_weights
        .iter_mut()
        .for_each(|w| *w = rng.spread(0.1));
    net.input_biases
        .iter_mut()
        .for_each(|w| *w = rng.spread(0.1));
    net.output_weights
        .iter_mut()
        .for_each(|w| *w = rng.spread(0.5));
    net.output_bias = rng.spread(0.1);
    net
}

/// The positions of `games` random games of up to 60 plies, each with a legal move.
fn random_positions(seed: u64, games: usize) -> Vec<Position> {
    let mut rng = Rng::new(seed);
    let mut positions = Vec::new();
    for _ in 0..games {
        let mut pos = Position::from_fen(STARTING_FEN).unwrap();
        for _ in 0..60 {
            let moves = pos.legal_moves();
            if moves.is_empty() {
                break;
            }
            positions.push(pos.clone());
            pos.make_move(moves[rng.below(moves.len())]);
        }
    }
    positions
}

/// Records of random games' positions, with random scores and results.
pub fn random_records(seed: u64, games: usize) -> Vec<Record> {
    let mut rng = Rng::new(seed);
    random_positions(seed, games)
        .iter()
        .map(|pos| {
            let score = rng.spread(600.0) as i32;
            let result =
                [GameResult::BlackWin, GameResult::Draw, GameResult::WhiteWin][rng.below(3)];
            Record::new(pos, score, result, pos.legal_moves()[0]).unwrap()
        })
        .collect()
}

/// Records of random games' positions scored by `teacher` (from White's side), as drawn games:
/// with a result weight of 0, a student network can learn them exactly.
pub fn teacher_records(teacher: &FloatNetwork, seed: u64, games: usize) -> Vec<Record> {
    random_positions(seed, games)
        .iter()
        .map(|pos| {
            let for_mover = teacher.evaluate(pos).round() as i32;
            let score = match pos.side_to_move() {
                cb2026_chess::Color::White => for_mover,
                cb2026_chess::Color::Black => -for_mover,
            };
            Record::new(pos, score, GameResult::Draw, pos.legal_moves()[0]).unwrap()
        })
        .collect()
}
