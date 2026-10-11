//! Playing a move on a position.
//!
//! [`Position::make_move`] changes the position in place. To explore a move and come back,
//! callers copy the position first ("copy-make"): `let mut child = pos.clone();
//! child.make_move(mv);`. A `Position` is a few hundred bytes, so copying is cheap and the code
//! stays simple. Stage 4 revisits this when it adds Zobrist hashing and the search's needs.
//!
//! See the Chess Programming Wiki, [Copy-Make](https://www.chessprogramming.org/Copy-Make).

use crate::attacks::pawn_attacks;
use crate::castling::CastlingSide;
use crate::movegen::castling_destinations;
use crate::moves::{Move, MoveKind};
use crate::piece::{Color, Piece, PieceType};
use crate::position::Position;
use crate::square::{Rank, Square};

impl Position {
    // ANCHOR: make_move
    /// Plays `mv`, which must be legal in this position (for example one from
    /// [`Position::legal_moves`]).
    pub fn make_move(&mut self, mv: Move) {
        let us = self.side_to_move();
        let them = !us;
        let from = mv.from();
        let to = mv.to();
        let mut halfmove_clock = self.halfmove_clock() + 1;
        let mut en_passant = None;

        if mv.kind() == MoveKind::Castle {
            // `to` is the castling rook. Lift both pieces, then put them on their destinations
            // (in Chess960 a destination may be the other piece's starting square).
            let side = if to.file() > from.file() {
                CastlingSide::KingSide
            } else {
                CastlingSide::QueenSide
            };
            let (king_to, rook_to) = castling_destinations(us, side);
            let king = self.remove_piece(from).expect("castling king");
            let rook = self.remove_piece(to).expect("castling rook");
            self.put_piece(king_to, king);
            self.put_piece(rook_to, rook);
            self.remove_castling_color(us);
        } else {
            let piece = self.remove_piece(from).expect("a piece on the from-square");

            if mv.is_capture() {
                // En passant captures the pawn beside the destination, not on it.
                let captured_square = match mv.kind() {
                    MoveKind::EnPassant => Square::new(to.file(), from.rank()),
                    _ => to,
                };
                self.remove_piece(captured_square)
                    .expect("a piece to capture");
                // Capturing a castling rook removes that castling right.
                self.remove_rook_right(them, captured_square);
                halfmove_clock = 0;
            }

            let landing = match mv.promotion() {
                Some(piece_type) => Piece::new(us, piece_type),
                None => piece,
            };
            self.put_piece(to, landing);

            match piece.piece_type {
                PieceType::Pawn => {
                    halfmove_clock = 0;
                    if mv.kind() == MoveKind::DoublePawnPush {
                        // Record the skipped square only if an enemy pawn could capture there,
                        // so equal positions always look equal.
                        let skipped = Square::new(from.file(), middle_rank(from.rank(), to.rank()));
                        if !(pawn_attacks(us, skipped) & self.pieces(them, PieceType::Pawn))
                            .is_empty()
                        {
                            en_passant = Some(skipped);
                        }
                    }
                }
                PieceType::King => self.remove_castling_color(us),
                PieceType::Rook => self.remove_rook_right(us, from),
                _ => {}
            }
        }

        let fullmove_number = self.fullmove_number() + u32::from(us == Color::Black);
        self.set_clocks(halfmove_clock, fullmove_number);
        self.set_en_passant(en_passant);
        self.set_side_to_move(them);
    }
    // ANCHOR_END: make_move

    /// Passes the turn without moving: a **null move**. It's illegal in chess, but the search
    /// uses it to ask "what if I could skip this move?" (null-move pruning). It must not be
    /// played while in check.
    ///
    /// The en passant square goes away, as after any move. The halfmove clock is reset, so the
    /// search's repetition check doesn't reach back across a move that wasn't really played.
    pub fn make_null_move(&mut self) {
        debug_assert!(
            !self.in_check(),
            "a null move in check would leave the king attacked"
        );
        let us = self.side_to_move();
        let fullmove_number = self.fullmove_number() + u32::from(us == Color::Black);
        self.set_clocks(0, fullmove_number);
        self.set_en_passant(None);
        self.set_side_to_move(!us);
    }

    /// Removes both of `color`'s castling rights.
    fn remove_castling_color(&mut self, color: Color) {
        let mut rights = self.castling();
        rights.remove_color(color);
        self.set_castling(rights);
    }

    /// Removes `color`'s castling right that uses the rook on `square`, if there is one.
    fn remove_rook_right(&mut self, color: Color, square: Square) {
        let back_rank = match color {
            Color::White => Rank::First,
            Color::Black => Rank::Eighth,
        };
        if square.rank() != back_rank {
            return;
        }
        for side in CastlingSide::ALL {
            if self.castling().rook_file(color, side) == Some(square.file()) {
                let mut rights = self.castling();
                rights.remove(color, side);
                self.set_castling(rights);
            }
        }
    }
}

/// The rank halfway between two ranks two apart (the square a double pawn push skips).
fn middle_rank(a: Rank, b: Rank) -> Rank {
    Rank::from_index((a.index() + b.index()) / 2).expect("between two ranks")
}

#[cfg(test)]
mod tests {
    use crate::{Color, Piece, PieceType, Position, Square};

    #[test]
    fn null_move_passes_the_turn_and_keeps_the_hash_right() {
        let mut pos =
            Position::from_fen("rnbqkbnr/ppp1pppp/8/8/3pP3/5N2/PPPP1PPP/RNBQKB1R b KQkq e3 0 3")
                .unwrap();
        let before = pos.clone();
        pos.make_null_move();
        assert_eq!(pos.side_to_move(), Color::White);
        assert_eq!(pos.en_passant(), None);
        assert_eq!(pos.halfmove_clock(), 0);
        assert_eq!(pos.fullmove_number(), 4);
        assert_eq!(pos.hash(), pos.compute_hash());
        assert!(pos.is_consistent());
        pos.make_null_move();
        // Same pieces, rights and side as before; only the clocks and en passant differ.
        assert_eq!(pos.occupied(), before.occupied());
        assert_eq!(pos.castling(), before.castling());
        assert_eq!(pos.side_to_move(), before.side_to_move());
        assert_eq!(pos.hash(), pos.compute_hash());
    }

    fn play(fen: &str, moves: &[&str]) -> Position {
        let mut pos = Position::from_fen(fen).unwrap();
        for uci in moves {
            let mv = pos
                .parse_uci_move(uci)
                .unwrap_or_else(|| panic!("{uci} is not legal in {}", pos.to_fen()));
            pos.make_move(mv);
            assert!(pos.is_consistent());
        }
        pos
    }

    #[test]
    fn opening_moves_update_everything() {
        let pos = play(crate::STARTING_FEN, &["e2e4", "c7c5", "g1f3"]);
        assert_eq!(
            pos.to_fen(),
            "rnbqkbnr/pp1ppppp/8/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
        );
    }

    #[test]
    fn en_passant_square_only_when_capturable() {
        let pos = play(crate::STARTING_FEN, &["e2e4"]);
        assert_eq!(pos.en_passant(), None, "no black pawn can take on e3");
        let pos = play(
            "rnbqkbnr/ppp1pppp/8/8/3p4/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            &["e2e4"],
        );
        assert_eq!(pos.en_passant(), Some(Square::E3));
    }

    #[test]
    fn en_passant_removes_the_captured_pawn() {
        let pos = play(
            "rnbqkbnr/ppp1pppp/8/8/3p4/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            &["e2e4", "d4e3"],
        );
        assert_eq!(pos.piece_at(Square::E4), None);
        assert_eq!(
            pos.piece_at(Square::E3),
            Some(Piece::new(Color::Black, PieceType::Pawn))
        );
    }

    #[test]
    fn castling_moves_king_and_rook() {
        let pos = play("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", &["e1g1", "e8c8"]);
        assert_eq!(pos.to_fen(), "2kr3r/8/8/8/8/8/8/R4RK1 w - - 2 2");
    }

    #[test]
    fn chess960_castling_where_destinations_overlap() {
        // King on f1, rook on g1: castling king side puts the king on g1 and the rook on f1,
        // swapping their squares.
        let pos = play("4k3/8/8/8/8/8/8/5KR1 w G - 0 1", &["f1g1"]);
        assert_eq!(pos.to_fen(), "4k3/8/8/8/8/8/8/5RK1 b - - 1 1");
    }

    #[test]
    fn rook_moves_and_captures_remove_castling_rights() {
        let pos = play("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", &["a1a8"]);
        assert_eq!(pos.to_fen(), "R3k2r/8/8/8/8/8/8/4K2R b Kk - 0 1");
    }

    #[test]
    fn promotion() {
        let pos = play("8/P6k/8/8/8/8/8/K7 w - - 0 1", &["a7a8n"]);
        assert_eq!(
            pos.piece_at(Square::A8),
            Some(Piece::new(Color::White, PieceType::Knight))
        );
    }
}
