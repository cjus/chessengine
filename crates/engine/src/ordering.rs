//! Move ordering: which moves to search first.
//!
//! Alpha-beta gives the same answer whatever order the moves come in, but its speed depends
//! hugely on that order. If the best move is searched first, every other move is refuted by
//! the first reply that's good enough, and the tree shrinks from about `b^d` positions toward
//! `b^(d/2)`. Searched last, it saves almost nothing.
//!
//! ## MVV-LVA
//!
//! Captures are the moves most likely to be best, and the cheapest good guess at which capture
//! is best is **Most Valuable Victim, Least Valuable Attacker**: take the biggest piece you
//! can, and among the ways to take it, use your smallest piece. Pawn takes queen comes first,
//! queen takes pawn last among captures. It ignores whether the capturing piece is defended
//! against; stage 14's static exchange evaluation (SEE) does better.
//!
//! Promotions to a queen are treated like captures of a queen-sized gain, so they're searched
//! early too. Quiet moves come after all of them, in generation order; stage 10 adds killer
//! moves and the history heuristic to order those.
//!
//! See the Chess Programming Wiki, [Move Ordering](https://www.chessprogramming.org/Move_Ordering)
//! and [MVV-LVA](https://www.chessprogramming.org/MVV-LVA).

use cb2026_chess::{Move, MoveKind, PieceType, Position};

// ANCHOR: mvv_lva
/// The MVV-LVA key of `mv` in `pos`: higher means search earlier. Quiet moves score 0;
/// captures and queen promotions score above 0, by victim (or promoted piece) first and by
/// attacker second.
///
/// The key uses the pieces' *rank* (pawn 1 … queen 5), not their values: only the order
/// matters, and ranks keep the victim strictly more important than the attacker.
pub fn mvv_lva(pos: &Position, mv: Move) -> i32 {
    let rank = |pt: PieceType| pt.index() as i32 + 1;
    let mut gain = 0;
    if mv.is_capture() {
        let victim = match mv.kind() {
            MoveKind::EnPassant => PieceType::Pawn,
            _ => pos
                .piece_at(mv.to())
                .map_or(PieceType::Pawn, |p| p.piece_type),
        };
        gain += rank(victim);
    }
    if mv.promotion() == Some(PieceType::Queen) {
        gain += rank(PieceType::Queen);
    }
    if gain == 0 {
        return 0;
    }
    let attacker = pos
        .piece_at(mv.from())
        .map_or(PieceType::Pawn, |p| p.piece_type);
    // Six attacker ranks fit between consecutive victim ranks, so the victim always decides.
    gain * 8 - rank(attacker)
}
// ANCHOR_END: mvv_lva

/// Sorts `moves` best first by [`mvv_lva`]. The sort is stable, so moves with equal keys (all
/// the quiet moves, for example) keep their generation order.
pub fn order_moves(pos: &Position, moves: &mut [Move]) {
    moves.sort_by_cached_key(|&mv| -mvv_lva(pos, mv));
}

/// Whether the quiescence search looks at `mv`: captures and queen promotions (but not
/// under-promotions, which almost never matter).
pub fn is_noisy(mv: Move) -> bool {
    match mv.promotion() {
        Some(piece) => piece == PieceType::Queen,
        None => mv.is_capture(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordered(fen: &str) -> Vec<String> {
        let pos = Position::from_fen(fen).unwrap();
        let mut moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        order_moves(&pos, &mut moves);
        moves.iter().map(|mv| pos.to_san(*mv)).collect()
    }

    #[test]
    fn biggest_victim_first_then_smallest_attacker() {
        // White can take the queen on d5 with the pawn or the rook, or the knight on f5 with
        // the pawn.
        let moves = ordered("4k3/8/8/3q1n2/4P3/8/8/3RK3 w - - 0 1");
        assert_eq!(&moves[..3], ["exd5", "Rxd5", "exf5"]);
    }

    #[test]
    fn queen_promotions_come_early_and_quiet_moves_last() {
        let pos = Position::from_fen("4k3/1P6/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let mut moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        order_moves(&pos, &mut moves);
        assert_eq!(pos.to_san(moves[0]), "b8=Q+");
        assert!(moves[1..].iter().all(|&mv| mvv_lva(&pos, mv) == 0));
    }

    #[test]
    fn en_passant_is_a_pawn_capture() {
        let pos = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2").unwrap();
        let ep = pos.parse_uci_move("e5d6").unwrap();
        assert_eq!(mvv_lva(&pos, ep), 8 - 1);
        assert!(is_noisy(ep));
    }

    #[test]
    fn under_promotions_are_quiet_for_quiescence() {
        let pos = Position::from_fen("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let knight = pos.parse_uci_move("b7b8n").unwrap();
        let queen = pos.parse_uci_move("b7b8q").unwrap();
        assert!(!is_noisy(knight));
        assert!(is_noisy(queen));
    }
}
