//! Perft: counting positions to prove move generation correct.
//!
//! `perft(position, depth)` counts the leaf positions of the full game tree to that depth:
//! every legal move, every reply, and so on. The counts for well-known test positions have
//! been computed by many independent programs, so a move generator that reproduces them
//! exactly is almost certainly right. A single missing en passant or a wrong castling right
//! changes the count.
//!
//! When a count is wrong, [`divide`] splits it by first move; comparing that with a trusted
//! engine's split points straight at the move whose subtree is wrong.
//!
//! **Bulk counting:** at depth 1 the count is just the number of legal moves, so there is no
//! need to play them. That one shortcut makes perft several times faster.
//!
//! See the Chess Programming Wiki, [Perft](https://www.chessprogramming.org/Perft) and
//! [Perft Results](https://www.chessprogramming.org/Perft_Results).

use crate::moves::Move;
use crate::position::Position;

// ANCHOR: perft
/// The number of leaf positions `depth` plies below `pos`.
pub fn perft(pos: &Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = pos.legal_moves();
    if depth == 1 {
        // Bulk counting: each legal move leads to exactly one leaf.
        return moves.len() as u64;
    }
    moves
        .iter()
        .map(|&mv| {
            let mut child = pos.clone();
            child.make_move(mv);
            perft(&child, depth - 1)
        })
        .sum()
}
// ANCHOR_END: perft

/// The perft count split by first move, for finding which move's subtree is wrong.
pub fn divide(pos: &Position, depth: u32) -> Vec<(Move, u64)> {
    pos.legal_moves()
        .iter()
        .map(|&mv| {
            let mut child = pos.clone();
            child.make_move(mv);
            (mv, perft(&child, depth.saturating_sub(1)))
        })
        .collect()
}

/// One position from a perft suite: a FEN and the expected count at each depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerftCase {
    /// The position.
    pub fen: String,
    /// `(depth, expected count)` pairs.
    pub counts: Vec<(u32, u64)>,
}

/// Parses a perft suite in the common EPD-like format, one position per line:
///
/// ```text
/// rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1 ;D1 20 ;D2 400 ;D3 8902
/// ```
///
/// Blank lines and lines starting with `#` are skipped. Lines that don't parse are skipped
/// too, so the caller should check it got the number of cases it expects.
pub fn parse_suite(text: &str) -> Vec<PerftCase> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split(';');
            let fen = parts.next()?.trim().to_string();
            let counts = parts
                .map(|part| {
                    let (depth, count) = part.trim().strip_prefix('D')?.split_once(' ')?;
                    Some((depth.parse().ok()?, count.trim().parse().ok()?))
                })
                .collect::<Option<Vec<(u32, u64)>>>()?;
            Some(PerftCase { fen, counts })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks every depth whose expected count is at most `max_nodes`, so the unit tests stay
    /// fast; `cb2026 perft` (or the release-mode perft test) runs the deep counts.
    fn check_suite(text: &str, expected_cases: usize, max_nodes: u64, chess960: bool) {
        let cases = parse_suite(text);
        assert_eq!(cases.len(), expected_cases, "every suite line should parse");
        let mut checked = 0;
        for case in &cases {
            let pos = Position::from_fen(&case.fen).unwrap_or_else(|e| panic!("{}: {e}", case.fen));
            for &(depth, expected) in case.counts.iter().filter(|(_, n)| *n <= max_nodes) {
                assert_eq!(
                    perft(&pos, depth),
                    expected,
                    "perft {depth} of {} (chess960: {chess960})",
                    case.fen
                );
                checked += 1;
            }
        }
        assert!(checked >= expected_cases, "at least one depth per position");
    }

    #[test]
    fn standard_suite() {
        check_suite(
            include_str!("../../../tests/data/perft.epd"),
            15,
            1_000_000,
            false,
        );
    }

    #[test]
    fn chess960_suite() {
        check_suite(
            include_str!("../../../tests/data/perft960.epd"),
            10,
            1_000_000,
            true,
        );
    }

    #[test]
    fn divide_adds_up() {
        let pos = Position::startpos();
        let split = divide(&pos, 3);
        assert_eq!(split.len(), 20);
        assert_eq!(split.iter().map(|(_, n)| n).sum::<u64>(), perft(&pos, 3));
    }

    #[test]
    fn parses_suite_lines() {
        let cases = parse_suite("# comment\n\n8/8/8/8/8/8/8/K6k w - - 0 1 ;D1 3 ;D2 9\n");
        assert_eq!(
            cases,
            vec![PerftCase {
                fen: "8/8/8/8/8/8/8/K6k w - - 0 1".to_string(),
                counts: vec![(1, 3), (2, 9)],
            }]
        );
    }
}
