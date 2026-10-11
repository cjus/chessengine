//! FEN parsing never panics, and any position it accepts survives a round trip and has a
//! working move generator.

#![no_main]

use cb2026_chess::Position;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let Ok(pos) = Position::from_fen(text) else {
        return;
    };
    let fen = pos.to_fen();
    let again = Position::from_fen(&fen).expect("a FEN we wrote reads back");
    assert_eq!(again.to_fen(), fen);
    assert_eq!(again.hash(), pos.hash());
    for &mv in pos.legal_moves().iter() {
        let mut child = pos.clone();
        child.make_move(mv);
        let _ = child.legal_moves();
    }
});
