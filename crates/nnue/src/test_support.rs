//! Helpers for this crate's tests: random networks, positions from random games, mirroring.

use cb2026_chess::{Position, STARTING_FEN};

use crate::float::FloatNetwork;
use crate::network::Activation;

/// A small seeded pseudo-random generator (xorshift64*), so every test run is the same.
pub struct Rng(u64);

impl Rng {
    /// A generator from `seed` (any value, zero included).
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// A number between `-limit` and `limit`.
    fn spread(&mut self, limit: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        (2.0 * unit - 1.0) * limit
    }
}

/// A float network with random weights of a realistic size: hidden values mostly between −1
/// and 1, so both sides of the clipping are exercised, and outputs of a few hundred centipawns.
pub fn random_float_network(hidden: usize, activation: Activation, seed: u64) -> FloatNetwork {
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

/// Every position of `games` random games of up to `plies` plies each. Every fourth game starts
/// from a Chess960 position.
pub fn random_game_positions(seed: u64, games: usize, plies: usize) -> Vec<Position> {
    let mut rng = Rng::new(seed);
    let mut positions = Vec::new();
    for game in 0..games {
        let mut pos = if game % 4 == 3 {
            Position::chess960(rng.below(960) as u32).expect("a Chess960 start")
        } else {
            Position::from_fen(STARTING_FEN).unwrap()
        };
        for _ in 0..plies {
            positions.push(pos.clone());
            let moves = pos.legal_moves();
            if moves.is_empty() {
                break;
            }
            pos.make_move(moves[rng.below(moves.len())]);
        }
    }
    positions
}

/// Positions with every kind of special move: castling on both sides for both colours (Chess960
/// too, where the king may stay put or land on its rook's square), en passant, and promotions
/// with and without a capture.
pub fn special_move_positions() -> Vec<Position> {
    [
        "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
        "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1",
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2",
        "4k3/8/8/8/3Pp3/8/8/4K3 b - d3 0 1",
        "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/p7/1N2K3 b - - 0 1",
        // Chess960: the king on g1 stays put castling king side; the king on f1 lands on its
        // rook's square g1.
        "4k3/8/8/8/8/8/8/R5KR w KQ - 0 1",
        "4k3/8/8/8/8/8/8/5KR1 w K - 0 1",
        "5kr1/8/8/8/8/8/8/4K3 b k - 0 1",
    ]
    .iter()
    .map(|fen| Position::from_fen(fen).unwrap_or_else(|e| panic!("{fen}: {e}")))
    .collect()
}

/// `pos` with the colours swapped and the board flipped top to bottom. Castling rights and the
/// en passant square are dropped: the network's inputs don't see them.
pub fn mirror(pos: &Position) -> Position {
    let fen = pos.to_fen();
    let fields: Vec<&str> = fen.split(' ').collect();
    let swap_case = |rank: &str| -> String {
        rank.chars()
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
    let mirrored = format!("{} {side} - - {} {}", board.join("/"), fields[4], fields[5]);
    Position::from_fen(&mirrored).unwrap_or_else(|e| panic!("{mirrored}: {e}"))
}
