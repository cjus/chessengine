//! Static exchange evaluation: what a capture wins once all the recaptures are played out.
//!
//! MVV-LVA (chapter 7) only looks at the first capture: pawn takes queen is good, queen takes
//! pawn is bad. But QxP is fine if the pawn is undefended, and NxB can lose a knight for a
//! bishop if the bishop is defended twice. **Static exchange evaluation** (SEE) plays out the
//! whole sequence of captures on the target square, each side always recapturing with its
//! least valuable attacker, and either side free to stop when continuing would lose material.
//! It looks only at that one square, with no search, so it is fast.
//!
//! The **swap algorithm**: record the material gained after each capture, assuming the side
//! that just captured is captured back. Then work backwards from the end: at each step the
//! side to capture chooses between stopping and taking, so the value at each step is the
//! better of the two.
//!
//! Captures that "uncover" an attacker behind the capturing piece (a rook behind a rook, a
//! bishop behind a queen) are handled by recomputing the attackers with the captured pieces
//! removed from the occupancy, which reveals **x-ray** attackers. Pins are ignored: SEE may
//! count a pinned piece as an attacker. Engines accept that small error for the speed.
//!
//! See the Chess Programming Wiki, [Static Exchange Evaluation](https://www.chessprogramming.org/Static_Exchange_Evaluation)
//! and [SEE - The Swap Algorithm](https://www.chessprogramming.org/SEE_-_The_Swap_Algorithm).

use cb2026_chess::{Bitboard, Color, Move, MoveKind, PieceType, Position, Square};

/// Piece values for exchanges, by [`PieceType::index`]. The king is worth more than anything,
/// so it only ever captures last (when nothing can take it back).
pub const SEE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 20_000];

// ANCHOR: see
/// The material `mv` wins (or loses, if negative) for the side making it, once the exchange
/// on its destination square is played out. A quiet move scores 0 or less: less if the piece
/// can be taken for free or for less than it is worth.
pub fn see(pos: &Position, mv: Move) -> i32 {
    let to = mv.to();
    let from = mv.from();
    let value = |pt: PieceType| SEE_VALUES[pt.index()];

    // What the first move captures, and the piece that will stand on the square after it.
    let mut occupied = pos.occupied().without(from);
    let first_gain = match mv.kind() {
        MoveKind::EnPassant => {
            let captured = Square::new(to.file(), from.rank());
            occupied = occupied.without(captured);
            value(PieceType::Pawn)
        }
        MoveKind::Castle => return 0,
        _ => pos.piece_at(to).map_or(0, |p| value(p.piece_type)),
    };
    let mover = pos.piece_at(from).expect("a piece on the from-square");
    let mut on_square = match mv.promotion() {
        Some(pt) => pt,
        None => mover.piece_type,
    };
    let promotion_gain = mv
        .promotion()
        .map_or(0, |pt| value(pt) - value(PieceType::Pawn));

    // gains[i]: material for the side that made capture i, if capture i is the last one.
    let mut gains = vec![first_gain + promotion_gain];
    let mut side = !mover.color;
    let mut attackers = pos.attackers_to(to, occupied) & occupied;
    loop {
        let ours = attackers & pos.pieces_of_color(side);
        let Some((piece_type, square)) = least_valuable(pos, ours) else {
            break;
        };
        // This side captures the piece standing on the square.
        let previous = *gains.last().expect("at least one entry");
        gains.push(value(on_square) - previous);
        on_square = piece_type;
        occupied = occupied.without(square);
        // Recompute, so sliders behind the piece that just moved join in (x-rays).
        attackers = pos.attackers_to(to, occupied) & occupied;
        side = !side;
        // A king can't capture into an attacked square: if it just did and the other side
        // has an attacker left, the king's capture was illegal. Drop it and stop.
        if piece_type == PieceType::King && !(attackers & pos.pieces_of_color(side)).is_empty() {
            gains.pop();
            break;
        }
    }

    // Work backwards: each side may decline to continue the exchange.
    while gains.len() > 1 {
        let last = gains.pop().expect("more than one entry");
        let before = gains.last_mut().expect("at least one entry");
        *before = (*before).min(-last);
    }
    gains[0]
}
// ANCHOR_END: see

/// The least valuable piece among `candidates`, with its square.
fn least_valuable(pos: &Position, candidates: Bitboard) -> Option<(PieceType, Square)> {
    PieceType::ALL.iter().find_map(|&pt| {
        let of_type = candidates & (pos.pieces(Color::White, pt) | pos.pieces(Color::Black, pt));
        of_type.first().map(|square| (pt, square))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn see_uci(fen: &str, uci: &str) -> i32 {
        let pos = Position::from_fen(fen).unwrap();
        let mv = pos
            .parse_uci_move(uci)
            .unwrap_or_else(|| panic!("{uci} is not legal"));
        see(&pos, mv)
    }

    #[test]
    fn undefended_piece_is_won() {
        assert_eq!(see_uci("4k3/8/8/3n4/8/8/8/3RK3 w - - 0 1", "d1d5"), 320);
    }

    #[test]
    fn defended_pawn_costs_the_queen() {
        // Qxd5 exd5: +100 - 900.
        assert_eq!(see_uci("4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1", "d1d5"), -800);
    }

    #[test]
    fn exchange_with_x_rays() {
        // Rxd5 Rxd5 Rxd5: White's second rook stands behind the first.
        // White wins the knight and a rook for a rook: +320.
        assert_eq!(see_uci("3rk3/8/8/3n4/8/8/3R4/3RK3 w - - 0 1", "d2d5"), 320);
    }

    #[test]
    fn a_side_stops_when_recapturing_loses() {
        // Bxe5 (pawn defended by a pawn): +100, then dxe5 wins the bishop: 100 - 330.
        assert_eq!(see_uci("4k3/8/3p4/4p3/8/2B5/8/4K3 w - - 0 1", "c3e5"), -230);
        // Pawn takes a pawn defended by the queen: the queen won't recapture, since the
        // c-pawn would take it.
        assert_eq!(see_uci("4k3/8/4q3/3p4/2P1P3/8/8/7K w - - 0 1", "e4d5"), 100);
    }

    #[test]
    fn quiet_moves_to_attacked_squares() {
        // Nd5 is attacked by a pawn and not defended: the knight is lost.
        assert_eq!(see_uci("4k3/8/4p3/8/8/2N5/8/4K3 w - - 0 1", "c3d5"), -320);
        // Nf3 is safe.
        assert_eq!(see_uci(cb2026_chess::STARTING_FEN, "g1f3"), 0);
    }

    #[test]
    fn en_passant_and_promotion() {
        assert_eq!(see_uci("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2", "e5d6"), 100);
        // Promoting on an attacked square: the new queen is taken by the rook.
        assert_eq!(see_uci("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8q"), -100);
    }
}
