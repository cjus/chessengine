//! Move parsing (SAN and UCI notation) never panics, and every move it accepts is legal and
//! writes back to text that reads as the same move.
//!
//! The input picks a position (one of a few, or a FEN before a newline) and then gives the
//! move text.

#![no_main]

use cb2026_chess::Position;
use libfuzzer_sys::fuzz_target;

const POSITIONS: [&str; 4] = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r1bqkbnr/pP1ppppp/8/8/8/8/P1PPPPPP/RNBQKBNR w KQkq - 0 1",
];

fuzz_target!(|text: &str| {
    let (pos, text) = match text.split_once('\n') {
        Some((fen, rest)) => match Position::from_fen(fen) {
            Ok(pos) => (pos, rest),
            Err(_) => return,
        },
        None => {
            let index = text.len() % POSITIONS.len();
            (Position::from_fen(POSITIONS[index]).unwrap(), text)
        }
    };
    let legal = pos.legal_moves();
    if let Ok(mv) = pos.parse_san(text) {
        assert!(legal.contains(&mv), "{text} parsed to an illegal move");
        assert_eq!(pos.parse_san(&pos.to_san(mv)), Ok(mv));
    }
    if let Some(mv) = pos.parse_uci_move(text) {
        assert!(legal.contains(&mv), "{text} parsed to an illegal move");
        assert_eq!(pos.parse_uci_move(&mv.to_uci(false)), Some(mv));
    }
});
