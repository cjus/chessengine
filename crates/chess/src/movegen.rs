//! Legal move generation.
//!
//! ## Legal, not pseudo-legal
//!
//! The simplest move generator produces *pseudo-legal* moves (each piece's moves, ignoring
//! whether they leave the king in check) and lets the caller try each one and throw away the
//! illegal ones. cb2026 generates only *legal* moves directly, using two pieces of information
//! computed once per position:
//!
//! - **Checkers**: the enemy pieces giving check. With two checkers only the king can move.
//!   With one, every other move must capture the checker or block the line between it and the
//!   king, so their destinations are limited to a **check mask**.
//! - **Pinned pieces**: our pieces that stand alone between our king and an enemy slider. A
//!   pinned piece may only move along the line of the pin.
//!
//! The king itself must not step onto an attacked square; when checking that, the king is
//! removed from the board first, so it can't "hide" from a slider behind its own square.
//!
//! Two moves are rare and subtle enough that they are checked by trying them instead: en
//! passant (removing two pawns from one rank can expose the king along that rank) and castling
//! (where in Chess960 the castling rook itself might be the piece shielding the king).
//!
//! See the Chess Programming Wiki, [Move Generation](https://www.chessprogramming.org/Move_Generation),
//! [Legal Move Generation](https://www.chessprogramming.org/Move_Generation#Legal) and
//! [Pin](https://www.chessprogramming.org/Pin).

use crate::attacks::{
    between, bishop_attacks, king_attacks, knight_attacks, line, pawn_attacks, queen_attacks,
    rook_attacks,
};
use crate::bitboard::Bitboard;
use crate::castling::CastlingSide;
use crate::moves::{Move, MoveKind, MoveList};
use crate::piece::{Color, PieceType};
use crate::position::Position;
use crate::square::{File, Rank, Square};

/// A function giving a sliding piece's attacks from a square, given the occupied squares.
type SliderAttacks = fn(Square, Bitboard) -> Bitboard;

/// Pieces a pawn can promote to, most valuable first.
const PROMOTION_PIECES: [PieceType; 4] = [
    PieceType::Queen,
    PieceType::Rook,
    PieceType::Bishop,
    PieceType::Knight,
];

/// The square a castling king lands on and the square its rook lands on.
///
/// These are the same in Chess960 as in standard chess: g1/f1 for the king side, c1/d1 for the
/// queen side (rank 8 for Black).
pub fn castling_destinations(color: Color, side: CastlingSide) -> (Square, Square) {
    let rank = match color {
        Color::White => Rank::First,
        Color::Black => Rank::Eighth,
    };
    match side {
        CastlingSide::KingSide => (Square::new(File::G, rank), Square::new(File::F, rank)),
        CastlingSide::QueenSide => (Square::new(File::C, rank), Square::new(File::D, rank)),
    }
}

impl Position {
    // ANCHOR: attackers_to
    /// All pieces, of both colours, that attack `square`, if the occupied squares were
    /// `occupied`.
    ///
    /// The trick: put each kind of piece on `square` and see what it would attack. A knight on
    /// `square` attacks exactly the squares from which a knight attacks `square`, and the same
    /// holds for every piece (with the pawn directions swapped).
    pub fn attackers_to(&self, square: Square, occupied: Bitboard) -> Bitboard {
        let rooks = self.pieces_of_type(PieceType::Rook) | self.pieces_of_type(PieceType::Queen);
        let bishops =
            self.pieces_of_type(PieceType::Bishop) | self.pieces_of_type(PieceType::Queen);
        (pawn_attacks(Color::White, square) & self.pieces(Color::Black, PieceType::Pawn))
            | (pawn_attacks(Color::Black, square) & self.pieces(Color::White, PieceType::Pawn))
            | (knight_attacks(square) & self.pieces_of_type(PieceType::Knight))
            | (king_attacks(square) & self.pieces_of_type(PieceType::King))
            | (rook_attacks(square, occupied) & rooks)
            | (bishop_attacks(square, occupied) & bishops)
    }
    // ANCHOR_END: attackers_to

    /// Whether any piece of `by` attacks `square`.
    pub fn is_attacked(&self, square: Square, by: Color) -> bool {
        !(self.attackers_to(square, self.occupied()) & self.pieces_of_color(by)).is_empty()
    }

    /// The enemy pieces giving check to the side to move.
    pub fn checkers(&self) -> Bitboard {
        let us = self.side_to_move();
        self.attackers_to(self.king_square(us), self.occupied()) & self.pieces_of_color(!us)
    }

    /// Whether the side to move is in check.
    pub fn in_check(&self) -> bool {
        !self.checkers().is_empty()
    }

    /// The side to move's pieces that are pinned to their king.
    pub fn pinned(&self) -> Bitboard {
        let us = self.side_to_move();
        let them = !us;
        let king = self.king_square(us);
        // Enemy sliders that would see our king on an empty board ("snipers").
        let snipers = (rook_attacks(king, Bitboard::EMPTY)
            & (self.pieces(them, PieceType::Rook) | self.pieces(them, PieceType::Queen)))
            | (bishop_attacks(king, Bitboard::EMPTY)
                & (self.pieces(them, PieceType::Bishop) | self.pieces(them, PieceType::Queen)));
        let mut pinned = Bitboard::EMPTY;
        for sniper in snipers {
            let blockers = between(king, sniper) & self.occupied();
            // Exactly one piece in the way, and it's ours: it is pinned.
            if blockers.count() == 1 {
                pinned |= blockers & self.pieces_of_color(us);
            }
        }
        pinned
    }

    /// All legal moves in this position.
    ///
    /// ```
    /// use cb2026_chess::Position;
    ///
    /// assert_eq!(Position::startpos().legal_moves().len(), 20);
    /// ```
    pub fn legal_moves(&self) -> MoveList {
        let mut list = MoveList::new();
        self.generate(&mut list);
        list
    }

    // ANCHOR: generate
    fn generate(&self, list: &mut MoveList) {
        let us = self.side_to_move();
        let them = !us;
        let own = self.pieces_of_color(us);
        let enemy = self.pieces_of_color(them);
        let occupied = self.occupied();
        let king = self.king_square(us);
        let checkers = self.checkers();

        // King moves: never onto a square the enemy attacks. Lift the king off the board
        // first, or it would seem to block a slider's attack on the square behind it.
        let without_king = occupied.without(king);
        for to in king_attacks(king) & !own {
            if (self.attackers_to(to, without_king) & enemy).is_empty() {
                list.push(Move::new(king, to, capture_or_quiet(enemy, to)));
            }
        }

        // In double check, only the king can move.
        if checkers.count() > 1 {
            return;
        }

        // With one checker, other moves must capture it or block the check.
        let check_mask = match checkers.first() {
            Some(checker) => between(king, checker).with(checker),
            None => Bitboard::ALL,
        };
        let pinned = self.pinned();
        // A pinned piece may only move along the line through its king and itself.
        let pin_mask = |from: Square| {
            if pinned.contains(from) {
                line(king, from)
            } else {
                Bitboard::ALL
            }
        };
        // ANCHOR_END: generate

        // Knights. A pinned knight never has a legal move: no knight move stays on a line
        // through its own square, and the pin mask takes care of that by itself.
        for from in self.pieces(us, PieceType::Knight) {
            for to in knight_attacks(from) & !own & check_mask & pin_mask(from) {
                list.push(Move::new(from, to, capture_or_quiet(enemy, to)));
            }
        }

        // Sliding pieces.
        let sliders: [(PieceType, SliderAttacks); 3] = [
            (PieceType::Bishop, bishop_attacks),
            (PieceType::Rook, rook_attacks),
            (PieceType::Queen, queen_attacks),
        ];
        for (piece_type, attacks) in sliders {
            for from in self.pieces(us, piece_type) {
                for to in attacks(from, occupied) & !own & check_mask & pin_mask(from) {
                    list.push(Move::new(from, to, capture_or_quiet(enemy, to)));
                }
            }
        }

        self.generate_pawn_moves(list, check_mask, &pin_mask);

        if checkers.is_empty() {
            self.generate_castling(list);
        }
    }

    fn generate_pawn_moves(
        &self,
        list: &mut MoveList,
        check_mask: Bitboard,
        pin_mask: &dyn Fn(Square) -> Bitboard,
    ) {
        let us = self.side_to_move();
        let enemy = self.pieces_of_color(!us);
        let occupied = self.occupied();
        let (forward, start_rank, last_rank): (i8, Rank, Rank) = match us {
            Color::White => (1, Rank::Second, Rank::Eighth),
            Color::Black => (-1, Rank::Seventh, Rank::First),
        };
        let ahead = |square: Square| {
            Rank::from_index((square.rank().index() as i8 + forward) as usize)
                .map(|rank| Square::new(square.file(), rank))
        };

        for from in self.pieces(us, PieceType::Pawn) {
            let allowed = check_mask & pin_mask(from);

            // Pushes: one square, or two from the starting rank, onto empty squares.
            if let Some(one) = ahead(from).filter(|s| !occupied.contains(*s)) {
                if allowed.contains(one) {
                    push_pawn_move(list, from, one, false, last_rank);
                }
                if from.rank() == start_rank
                    && let Some(two) = ahead(one).filter(|s| !occupied.contains(*s))
                    && allowed.contains(two)
                {
                    list.push(Move::new(from, two, MoveKind::DoublePawnPush));
                }
            }

            // Captures.
            for to in pawn_attacks(us, from) & enemy & allowed {
                push_pawn_move(list, from, to, true, last_rank);
            }

            // En passant: check it by trying it, since it removes two pawns from one rank.
            if let Some(ep) = self.en_passant()
                && pawn_attacks(us, from).contains(ep)
            {
                let mv = Move::new(from, ep, MoveKind::EnPassant);
                if self.leaves_king_safe(mv) {
                    list.push(mv);
                }
            }
        }
    }

    // ANCHOR: castling
    /// Castling moves, for standard chess and Chess960 alike.
    ///
    /// Castling needs: the right still held; every square the king and rook travel over or
    /// land on empty (apart from the king and rook themselves); and no square the king stands
    /// on, crosses or lands on attacked. The attack test runs with the king and castling rook
    /// lifted off the board, which catches the Chess960 case where the rook was shielding the
    /// king from an enemy rook or queen on the back rank.
    fn generate_castling(&self, list: &mut MoveList) {
        let us = self.side_to_move();
        let king = self.king_square(us);
        for side in CastlingSide::ALL {
            let Some(rook_file) = self.castling().rook_file(us, side) else {
                continue;
            };
            let rook = Square::new(rook_file, king.rank());
            let (king_to, rook_to) = castling_destinations(us, side);

            let king_path = between(king, king_to).with(king_to).with(king);
            let rook_path = between(rook, rook_to).with(rook_to);
            let movers = Bitboard::from_square(king).with(rook);
            if !((king_path | rook_path) & self.occupied() & !movers).is_empty() {
                continue;
            }
            let lifted = self.occupied() & !movers;
            let enemy = self.pieces_of_color(!us);
            let safe = king_path
                .squares()
                .all(|square| (self.attackers_to(square, lifted) & enemy).is_empty());
            if safe {
                list.push(Move::new(king, rook, MoveKind::Castle));
            }
        }
    }
    // ANCHOR_END: castling

    /// Plays `mv` on a copy of the position and checks that the mover's king isn't attacked.
    fn leaves_king_safe(&self, mv: Move) -> bool {
        let mut after = self.clone();
        after.make_move(mv);
        let us = self.side_to_move();
        !after.is_attacked(after.king_square(us), !us)
    }

    /// Finds the legal move written as `uci` in UCI notation, accepting both castling styles
    /// (`e1g1` and the Chess960 "king takes rook" `e1h1`).
    pub fn parse_uci_move(&self, uci: &str) -> Option<Move> {
        self.legal_moves()
            .iter()
            .copied()
            .find(|mv| mv.to_uci(false) == uci || mv.to_uci(true) == uci)
    }
}

/// A capture if an enemy piece stands on `to`, otherwise a quiet move.
fn capture_or_quiet(enemy: Bitboard, to: Square) -> MoveKind {
    if enemy.contains(to) {
        MoveKind::Capture
    } else {
        MoveKind::Quiet
    }
}

/// Adds a pawn move, expanding it into four promotions when it reaches the last rank.
fn push_pawn_move(list: &mut MoveList, from: Square, to: Square, capture: bool, last_rank: Rank) {
    if to.rank() == last_rank {
        for piece in PROMOTION_PIECES {
            list.push(Move::new(from, to, MoveKind::Promotion { piece, capture }));
        }
    } else {
        let kind = if capture {
            MoveKind::Capture
        } else {
            MoveKind::Quiet
        };
        list.push(Move::new(from, to, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves_uci(fen: &str) -> Vec<String> {
        let mut moves: Vec<String> = Position::from_fen(fen)
            .unwrap()
            .legal_moves()
            .iter()
            .map(|m| m.to_string())
            .collect();
        moves.sort();
        moves
    }

    #[test]
    fn start_position_has_twenty_moves() {
        assert_eq!(Position::startpos().legal_moves().len(), 20);
    }

    #[test]
    fn double_check_allows_only_king_moves() {
        // Black king e8 checked by the rook on e1 and the bishop on b5.
        let moves = moves_uci("4k3/8/8/1B6/8/8/8/4R1K1 b - - 0 1");
        assert!(moves.iter().all(|m| m.starts_with("e8")), "{moves:?}");
    }

    #[test]
    fn pinned_piece_stays_on_the_pin_line() {
        // The white rook on e2 is pinned by the rook on e8: it can only move along the e-file.
        let moves = moves_uci("4r1k1/8/8/8/8/8/4R3/4K3 w - - 0 1");
        let rook_moves: Vec<&String> = moves.iter().filter(|m| m.starts_with("e2")).collect();
        assert!(
            rook_moves.iter().all(|m| m.as_bytes()[2] == b'e'),
            "{rook_moves:?}"
        );
        assert!(
            moves.contains(&"e2e8".to_string()),
            "capturing the pinner is fine"
        );
    }

    #[test]
    fn en_passant_that_exposes_the_king_is_illegal() {
        // Capturing d5xe6 en passant would remove both pawns from rank 5 and expose the white
        // king on a5 to the rook on h5.
        let moves = moves_uci("4k3/8/8/K2pP2r/8/8/8/8 w - d6 0 1");
        assert!(!moves.contains(&"e5d6".to_string()), "{moves:?}");
    }

    #[test]
    fn castling_through_check_is_illegal() {
        // The bishop on c4 covers f1, so White can't castle king side but can castle queen side.
        let moves = moves_uci("r3k2r/8/8/8/2b5/8/8/R3K2R w KQkq - 0 1");
        assert!(!moves.contains(&"e1g1".to_string()));
        assert!(moves.contains(&"e1c1".to_string()));
    }

    #[test]
    fn promotions_come_in_fours() {
        let moves = moves_uci("8/P6k/8/8/8/8/8/K7 w - - 0 1");
        for piece in ["q", "r", "b", "n"] {
            assert!(moves.contains(&format!("a7a8{piece}")));
        }
    }

    #[test]
    fn parses_both_castling_notations() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let standard = pos.parse_uci_move("e1g1").unwrap();
        let king_takes_rook = pos.parse_uci_move("e1h1").unwrap();
        assert_eq!(standard, king_takes_rook);
        assert_eq!(standard.kind(), MoveKind::Castle);
        assert_eq!(pos.parse_uci_move("e1e3"), None);
    }
}
