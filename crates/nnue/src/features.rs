//! The network's inputs: which piece stands on which square, as numbers 0 to 767.
//!
//! ## 768 inputs, seen from each side
//!
//! An input is one fact about the board, such as "a white knight stands on f3". There are 2
//! colours × 6 piece types × 64 squares = 768 of them, and in a position one is on for each piece,
//! so at most 32 are. The network reads them twice, once from each side's point of view. From a
//! side's view its own pieces are "ours" and the opponent's "theirs", and Black sees the board
//! flipped top to bottom, so the same weights describe White's knight on f3 for White and Black's
//! knight on f6 for Black:
//!
//! ```text
//! input = 384 × (0 for ours, 1 for theirs) + 64 × piece type + square
//! ```
//!
//! The piece types run from pawn (0) to king (5) and the squares from a1 (0) to h8 (63), both
//! numbered as in `cb2026-chess`. From Black's side a square's rank is flipped: a1 becomes a8
//! (`square ^ 56`, [`Square::flip_rank`]). Flipping, not rotating, keeps the kings on the same
//! file for both sides.
//!
//! ## Why these inputs
//!
//! None of them depends on where a king stands, so no move ever forces the first layer to be
//! recomputed from scratch, and they need the least training data. Stronger networks also look
//! at the king's square ("king buckets"), at the cost of more data and of recomputing after many
//! king moves; cb2026 may add that later, measured like any other technique.
//!
//! ## What a move changes
//!
//! A move switches inputs off where pieces leave and on where they land ([`Changes`]):
//!
//! | Move | Off | On |
//! | --- | --- | --- |
//! | Quiet move or pawn push | the piece on its from-square | the piece on its to-square |
//! | Capture | the piece, and the captured piece | the piece on its to-square |
//! | En passant | the pawn, and the captured pawn beside the to-square | the pawn on its to-square |
//! | Promotion | the pawn (and any captured piece) | the new piece on its to-square |
//! | Castling | the king and the rook | the king and the rook on their new squares |
//!
//! So a move turns at most two inputs off and two on. In Chess960 a castling king or rook may
//! land on its own square, or on the other's; switching all the old squares off and all the new
//! ones on handles every case.
//!
//! See the Chess Programming Wiki, [NNUE](https://www.chessprogramming.org/NNUE).

use cb2026_chess::movegen::castling_destinations;
use cb2026_chess::{CastlingSide, Color, Move, MoveKind, Piece, Position, Square};

/// The number of squares.
const SQUARES: usize = 64;
/// The inputs for one side's pieces: 6 piece types × 64 squares.
const PER_SIDE: usize = 6 * SQUARES;
/// How many inputs the network has: ours and theirs, 6 piece types, 64 squares.
pub const INPUTS: usize = 2 * PER_SIDE;

// ANCHOR: index
/// The input for `piece` standing on `square`, seen from `view`'s side of the board.
pub fn index(view: Color, piece: Piece, square: Square) -> usize {
    let side = if piece.color == view { 0 } else { 1 };
    let square = match view {
        Color::White => square,
        Color::Black => square.flip_rank(),
    };
    side * PER_SIDE + piece.piece_type.index() * SQUARES + square.index()
}
// ANCHOR_END: index

/// The inputs that are on in `pos`, seen from `view`'s side: one for each piece.
pub fn active(pos: &Position, view: Color) -> impl Iterator<Item = usize> + '_ {
    pos.occupied().squares().map(move |square| {
        let piece = pos
            .piece_at(square)
            .expect("an occupied square has a piece");
        index(view, piece, square)
    })
}

// ANCHOR: changes
/// The pieces a move lifts and puts down, which are the inputs it switches off and on. They are
/// pieces and squares, not input numbers, so they serve both views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Changes {
    removed: [Option<(Piece, Square)>; 2],
    added: [Option<(Piece, Square)>; 2],
}

impl Changes {
    /// What playing `mv` changes in `pos`, the position before the move. `mv` must be legal in
    /// `pos`.
    pub fn of(pos: &Position, mv: Move) -> Changes {
        let us = pos.side_to_move();
        let (from, to) = (mv.from(), mv.to());
        let piece = pos.piece_at(from).expect("a piece on the from-square");

        if mv.kind() == MoveKind::Castle {
            // "King takes own rook": `to` is the rook's square, as in `Position::make_move`.
            let side = if to.file() > from.file() {
                CastlingSide::KingSide
            } else {
                CastlingSide::QueenSide
            };
            let (king_to, rook_to) = castling_destinations(us, side);
            let rook = pos.piece_at(to).expect("the castling rook");
            return Changes {
                removed: [Some((piece, from)), Some((rook, to))],
                added: [Some((piece, king_to)), Some((rook, rook_to))],
            };
        }

        let captured = mv.is_capture().then(|| {
            // En passant captures the pawn beside the destination, not on it.
            let square = match mv.kind() {
                MoveKind::EnPassant => Square::new(to.file(), from.rank()),
                _ => to,
            };
            (pos.piece_at(square).expect("a piece to capture"), square)
        });
        let landing = match mv.promotion() {
            Some(piece_type) => Piece::new(us, piece_type),
            None => piece,
        };
        Changes {
            removed: [Some((piece, from)), captured],
            added: [Some((landing, to)), None],
        }
    }

    /// The pieces lifted: their inputs go off.
    pub fn removed(&self) -> impl Iterator<Item = (Piece, Square)> + '_ {
        self.removed.iter().flatten().copied()
    }

    /// The pieces put down: their inputs come on.
    pub fn added(&self) -> impl Iterator<Item = (Piece, Square)> + '_ {
        self.added.iter().flatten().copied()
    }
}
// ANCHOR_END: changes

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use cb2026_chess::{PieceType, STARTING_FEN};

    use super::*;
    use crate::test_support::{random_game_positions, special_move_positions};

    fn sorted_active(pos: &Position, view: Color) -> Vec<usize> {
        let mut inputs: Vec<usize> = active(pos, view).collect();
        inputs.sort_unstable();
        inputs
    }

    #[test]
    fn every_piece_and_square_has_its_own_input() {
        for view in Color::ALL {
            let mut seen = HashSet::new();
            for color in Color::ALL {
                for piece_type in PieceType::ALL {
                    for square in Square::all() {
                        let input = index(view, Piece::new(color, piece_type), square);
                        assert!(input < INPUTS);
                        assert!(seen.insert(input), "{input} used twice");
                    }
                }
            }
            assert_eq!(seen.len(), INPUTS);
        }
    }

    #[test]
    fn black_sees_the_board_flipped_with_its_own_pieces_as_ours() {
        let f3 = Square::from_index(21).unwrap();
        let f6 = Square::from_index(45).unwrap();
        let white_knight = Piece::new(Color::White, PieceType::Knight);
        let black_knight = Piece::new(Color::Black, PieceType::Knight);
        assert_eq!(
            index(Color::White, white_knight, f3),
            index(Color::Black, black_knight, f6)
        );
        // Ours first: a knight is 64 × 1 past the pawns, f3 is square 21.
        assert_eq!(index(Color::White, white_knight, f3), 64 + 21);
        assert_eq!(index(Color::Black, white_knight, f3), 384 + 64 + 45);
    }

    #[test]
    fn the_start_position_has_32_inputs_on_from_each_side() {
        let pos = Position::from_fen(STARTING_FEN).unwrap();
        // The start position is symmetric, so both sides see the same inputs.
        assert_eq!(sorted_active(&pos, Color::White).len(), 32);
        assert_eq!(
            sorted_active(&pos, Color::White),
            sorted_active(&pos, Color::Black)
        );
    }

    #[test]
    fn changes_turn_the_inputs_before_a_move_into_those_after_it() {
        let mut kinds = HashSet::new();
        let positions = special_move_positions()
            .into_iter()
            .chain(random_game_positions(2026, 40, 160));
        for pos in positions {
            for &mv in pos.legal_moves().iter() {
                let mut child = pos.clone();
                child.make_move(mv);
                let changes = Changes::of(&pos, mv);
                assert!(changes.removed().count() <= 2 && changes.added().count() <= 2);
                for view in Color::ALL {
                    let mut inputs = sorted_active(&pos, view);
                    for (piece, square) in changes.removed() {
                        let at = inputs
                            .iter()
                            .position(|&i| i == index(view, piece, square))
                            .unwrap_or_else(|| {
                                panic!("{mv:?} in {}: nothing to remove", pos.to_fen())
                            });
                        inputs.remove(at);
                    }
                    inputs.extend(changes.added().map(|(p, s)| index(view, p, s)));
                    inputs.sort_unstable();
                    assert_eq!(
                        inputs,
                        sorted_active(&child, view),
                        "{mv:?} in {}",
                        pos.to_fen()
                    );
                }
                kinds.insert(match mv.kind() {
                    MoveKind::Promotion { capture, .. } => {
                        if capture {
                            "promotion capture"
                        } else {
                            "promotion"
                        }
                    }
                    MoveKind::Castle if mv.to().file() > mv.from().file() => "castle king side",
                    MoveKind::Castle => "castle queen side",
                    MoveKind::EnPassant => "en passant",
                    MoveKind::Capture => "capture",
                    MoveKind::DoublePawnPush => "double pawn push",
                    MoveKind::Quiet => "quiet",
                });
            }
        }
        // Every kind of move was checked.
        assert_eq!(kinds.len(), 8, "{kinds:?}");
    }
}
