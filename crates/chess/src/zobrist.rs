//! Zobrist hashing: a 64-bit fingerprint of a position.
//!
//! The search will meet the same position many times, reached by different move orders
//! ("transpositions"), and must also spot repeated positions for the threefold-repetition
//! rule. Comparing whole positions is slow, so each position gets a 64-bit **hash key**
//! instead.
//!
//! ## How it works
//!
//! Give every feature of a position a fixed random 64-bit number:
//!
//! - one per (piece, square): 12 × 64 numbers;
//! - one for "Black to move";
//! - one per castling right (colour × side);
//! - one per en passant file.
//!
//! The key is the XOR of the numbers for the features present. Because XOR undoes itself,
//! the key can be **updated incrementally**: moving a knight from g1 to f3 is
//! `key ^= KNIGHT_G1 ^ KNIGHT_F3`, a couple of instructions, instead of recomputing from
//! scratch.
//!
//! Different positions can in principle share a key, but with 64 random bits that's rare
//! enough that engines accept it (and the transposition table double-checks moves it gets
//! back).
//!
//! The numbers come from a fixed-seed generator at compile time, so keys are the same in every
//! build.
//!
//! See the Chess Programming Wiki, [Zobrist Hashing](https://www.chessprogramming.org/Zobrist_Hashing).

use crate::castling::{CastlingRights, CastlingSide};
use crate::piece::{Color, Piece};
use crate::square::{File, Square};

/// A compile-time pseudo-random generator (SplitMix64).
const fn splitmix(state: u64) -> (u64, u64) {
    let state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (state, z ^ (z >> 31))
}

/// All the random numbers, in one array so a single generator fills it.
const KEY_COUNT: usize = 12 * 64 + 1 + 4 + 8;

const KEYS: [u64; KEY_COUNT] = {
    let mut keys = [0u64; KEY_COUNT];
    let mut state = 0x00cb_2026_5eed_0001_u64;
    let mut i = 0;
    while i < KEY_COUNT {
        let (next_state, value) = splitmix(state);
        state = next_state;
        keys[i] = value;
        i += 1;
    }
    keys
};

const SIDE_INDEX: usize = 12 * 64;
const CASTLING_INDEX: usize = SIDE_INDEX + 1;
const EN_PASSANT_INDEX: usize = CASTLING_INDEX + 4;

// ANCHOR: keys
/// The key for `piece` standing on `square`.
pub fn piece_square(piece: Piece, square: Square) -> u64 {
    let piece_index = piece.color.index() * 6 + piece.piece_type.index();
    KEYS[piece_index * 64 + square.index()]
}

/// The key XORed in when Black is to move.
pub fn black_to_move() -> u64 {
    KEYS[SIDE_INDEX]
}

/// The combined key for a set of castling rights.
pub fn castling(rights: CastlingRights) -> u64 {
    let mut key = 0;
    for color in Color::ALL {
        for side in CastlingSide::ALL {
            if rights.has(color, side) {
                key ^= KEYS[CASTLING_INDEX + color.index() * 2 + side.index()];
            }
        }
    }
    key
}

/// The key for an en passant square on `file`.
pub fn en_passant(file: File) -> u64 {
    KEYS[EN_PASSANT_INDEX + file.index()]
}
// ANCHOR_END: keys

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Position;

    fn play(moves: &[&str]) -> Position {
        let mut pos = Position::startpos();
        for uci in moves {
            let mv = pos.parse_uci_move(uci).expect("legal move");
            pos.make_move(mv);
        }
        pos
    }

    /// Walks the whole tree to `depth`, checking the incremental key against a fresh one.
    fn check_tree(pos: &Position, depth: u32) {
        assert_eq!(pos.hash(), pos.compute_hash(), "{}", pos.to_fen());
        if depth == 0 {
            return;
        }
        for &mv in pos.legal_moves().iter() {
            let mut child = pos.clone();
            child.make_move(mv);
            check_tree(&child, depth - 1);
        }
    }

    #[test]
    fn incremental_keys_match_full_recomputation() {
        let suites = [
            include_str!("../../../tests/data/perft.epd"),
            include_str!("../../../tests/data/perft960.epd"),
        ];
        for case in suites.iter().flat_map(|s| crate::perft::parse_suite(s)) {
            let pos = Position::from_fen(&case.fen).unwrap();
            check_tree(&pos, 2);
        }
    }

    #[test]
    fn transpositions_have_equal_keys() {
        let a = play(&["g1f3", "g8f6", "b1c3"]);
        let b = play(&["b1c3", "g8f6", "g1f3"]);
        assert_eq!(a.hash(), b.hash());
        assert_eq!(a, b);
    }

    #[test]
    fn side_castling_and_en_passant_change_the_key() {
        let start = Position::startpos();
        // Same pieces as the start, but Black to move.
        let black =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1").unwrap();
        assert_ne!(start.hash(), black.hash());
        let no_castling =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1").unwrap();
        assert_ne!(start.hash(), no_castling.hash());
        // Knights out and back: the same position again, with the same key.
        let back = play(&["g1f3", "g8f6", "f3g1", "f6g8"]);
        assert_eq!(start.hash(), back.hash());
    }

    #[test]
    fn keys_are_distinct_and_nonzero() {
        let mut sorted = KEYS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), KEY_COUNT);
        assert!(!sorted.contains(&0));
    }
}
