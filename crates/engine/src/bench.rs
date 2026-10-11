//! The benchmark: a fixed search whose node count fingerprints the search.
//!
//! `cb2026 bench` searches a fixed list of positions to a fixed depth and reports the total
//! number of nodes and the speed. Because the search is deterministic, the node count only
//! changes when the search or evaluation changes. That makes it a cheap check:
//!
//! - every commit message ends with `Bench: <nodes>`;
//! - a change meant to be "no functional change" (a refactor, a speedup) must leave the
//!   number unchanged;
//! - two builds that print the same number search identically.
//!
//! See the Chess Programming Wiki, [Engine Testing](https://www.chessprogramming.org/Engine_Testing).

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use cb2026_chess::Position;

use crate::{Engine, Limits};

/// The depth every bench position is searched to. It was 4 until stage 14, when the pruning
/// made a depth-4 search too small to exercise the search.
pub const BENCH_DEPTH: u32 = 10;

/// The bench positions: openings, middlegames and endgames from the test suites.
pub const BENCH_POSITIONS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "r1bqkb1r/1ppp1ppp/p1n2n2/4p3/B3P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 2 5",
    "rnbqkb1r/pp2pppp/3p1n2/8/3NP3/8/PPP2PPP/RNBQKB1R w KQkq - 1 5",
    "rnbqk2r/ppp1bppp/4pn2/3p2B1/2PP4/2N5/PP2PPPP/R2QKBNR w KQkq - 2 5",
    "r1bq1rk1/pp2ppbp/2np1np1/8/3NP3/2N1BP2/PPPQ2PP/R3KB1R w KQ - 3 9",
    "2r3k1/pp3ppp/4p3/3p4/3P4/2P1P3/P4PPP/2R3K1 w - - 0 25",
    "8/8/4k3/8/2p5/8/B2P2K1/8 w - - 0 1",
    "8/5pk1/6p1/8/8/3B2P1/5PK1/8 w - - 0 40",
    "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 30",
];

/// Runs the benchmark: returns the total nodes and the time taken.
pub fn bench(engine: &mut Engine, depth: u32) -> (u64, Duration) {
    let stop = AtomicBool::new(false);
    let limits = Limits::depth(depth);
    let start = Instant::now();
    let mut nodes = 0;
    for fen in BENCH_POSITIONS {
        let pos = Position::from_fen(fen).expect("bench positions are valid");
        engine.new_game();
        let mut last = 0;
        engine.search(&pos, &[], &limits, &stop, &mut |info| last = info.nodes);
        nodes += last;
    }
    (nodes, start.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_positions_are_valid_and_playable() {
        for fen in BENCH_POSITIONS {
            let pos = Position::from_fen(fen).unwrap();
            assert!(!pos.legal_moves().is_empty(), "{fen}");
        }
    }

    #[test]
    fn bench_is_deterministic() {
        let a = bench(&mut Engine::new(), 2).0;
        let b = bench(&mut Engine::new(), 2).0;
        assert_eq!(a, b);
        assert!(a > 0);
    }
}
