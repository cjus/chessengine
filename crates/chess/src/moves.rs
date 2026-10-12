//! Moves and move lists.
//!
//! ## A move in 16 bits
//!
//! A move is packed into a `u16`:
//!
//! ```text
//!  15 ... 12 | 11 ... 6 | 5 ... 0
//!    kind    |    to    |  from
//! ```
//!
//! Sixteen bits is enough because the position supplies everything else (which piece moves,
//! what it captures). Small moves matter later: the transposition table stores one per entry,
//! and the search keeps thousands of them in killer and history tables.
//!
//! ## Castling is "king takes own rook"
//!
//! A castling move is stored with `from` = the king's square and `to` = the castling rook's
//! square. That describes castling unambiguously in Chess960 too, where the king's destination
//! alone may not (the king might not move at all). [`Move::to_uci`] converts it to the usual
//! notation (`e1g1`) for standard chess.
//!
//! See the Chess Programming Wiki, [Encoding Moves](https://www.chessprogramming.org/Encoding_Moves).

use std::fmt;
use std::ops::Deref;

use crate::piece::PieceType;
use crate::square::{File, Square};

/// What kind of move a [`Move`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveKind {
    /// A quiet move: no capture, no special rule.
    Quiet,
    /// A pawn moving two squares from its starting rank.
    DoublePawnPush,
    /// Castling (`from` = king, `to` = castling rook).
    Castle,
    /// A capture (not en passant).
    Capture,
    /// An en passant capture.
    EnPassant,
    /// A pawn promotion, possibly with a capture.
    Promotion {
        /// The piece the pawn becomes.
        piece: PieceType,
        /// Whether the promotion also captures.
        capture: bool,
    },
}

impl MoveKind {
    /// The 4-bit code stored in a [`Move`].
    const fn code(self) -> u16 {
        match self {
            MoveKind::Quiet => 0,
            MoveKind::DoublePawnPush => 1,
            MoveKind::Castle => 2,
            MoveKind::Capture => 4,
            MoveKind::EnPassant => 5,
            MoveKind::Promotion { piece, capture } => {
                let piece_code = match piece {
                    PieceType::Knight => 0,
                    PieceType::Bishop => 1,
                    PieceType::Rook => 2,
                    // Pawns and kings can't be promoted to; treat anything else as a queen.
                    _ => 3,
                };
                8 | if capture { 4 } else { 0 } | piece_code
            }
        }
    }

    const fn from_code(code: u16) -> MoveKind {
        match code {
            0 => MoveKind::Quiet,
            1 => MoveKind::DoublePawnPush,
            2 => MoveKind::Castle,
            4 => MoveKind::Capture,
            5 => MoveKind::EnPassant,
            8..=15 => MoveKind::Promotion {
                piece: match code & 3 {
                    0 => PieceType::Knight,
                    1 => PieceType::Bishop,
                    2 => PieceType::Rook,
                    _ => PieceType::Queen,
                },
                capture: code & 4 != 0,
            },
            // Codes 3, 6 and 7 are never written.
            _ => MoveKind::Quiet,
        }
    }
}

// ANCHOR: move
/// A move, packed into 16 bits (see the [module docs](self)).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Move(u16);

impl Move {
    /// A move from `from` to `to` of the given kind.
    pub const fn new(from: Square, to: Square, kind: MoveKind) -> Move {
        Move(from.index() as u16 | (to.index() as u16) << 6 | kind.code() << 12)
    }
    // ANCHOR_END: move

    /// The move's 16-bit encoding, for compact storage (the transposition table).
    pub const fn to_bits(self) -> u16 {
        self.0
    }

    /// The move with encoding `bits`, as made by [`Move::to_bits`]. Any value decodes to some
    /// move, so callers must check it is legal before playing it.
    pub const fn from_bits(bits: u16) -> Move {
        Move(bits)
    }

    /// The square the moving piece starts on (the king's square for castling).
    pub const fn from(self) -> Square {
        match Square::from_index((self.0 & 63) as usize) {
            Some(square) => square,
            None => unreachable!(),
        }
    }

    /// The square the moving piece lands on (the castling rook's square for castling).
    pub const fn to(self) -> Square {
        match Square::from_index(((self.0 >> 6) & 63) as usize) {
            Some(square) => square,
            None => unreachable!(),
        }
    }

    /// The kind of move.
    pub const fn kind(self) -> MoveKind {
        MoveKind::from_code(self.0 >> 12)
    }

    /// Whether the move captures something (including en passant and capturing promotions).
    pub const fn is_capture(self) -> bool {
        matches!(
            self.kind(),
            MoveKind::Capture | MoveKind::EnPassant | MoveKind::Promotion { capture: true, .. }
        )
    }

    /// The piece a promotion creates, if this is a promotion.
    pub const fn promotion(self) -> Option<PieceType> {
        match self.kind() {
            MoveKind::Promotion { piece, .. } => Some(piece),
            _ => None,
        }
    }

    /// The move in UCI notation, like `e2e4` or `e7e8q`.
    ///
    /// Castling is written as the king's two-square move (`e1g1`) in standard chess, and as
    /// "king takes rook" (`e1h1`) when `chess960` is set, as the UCI protocol requires.
    pub fn to_uci(self, chess960: bool) -> String {
        let from = self.from();
        let mut to = self.to();
        if self.kind() == MoveKind::Castle && !chess960 {
            let file = if to.file() > from.file() {
                File::G
            } else {
                File::C
            };
            to = Square::new(file, from.rank());
        }
        let mut s = format!("{from}{to}");
        if let Some(piece) = self.promotion() {
            s.push(piece.to_char());
        }
        s
    }
}

impl fmt::Display for Move {
    /// UCI notation for standard chess (see [`Move::to_uci`]).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_uci(false))
    }
}

impl fmt::Debug for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}({:?})", self.from(), self.to(), self.kind())
    }
}

/// The most legal moves any chess position has is 218; this leaves headroom.
pub const MAX_MOVES: usize = 256;

/// A list of moves stored inline, without heap allocation.
///
/// Move generation runs millions of times per second during search, so the list lives on the
/// stack instead of in a `Vec`.
#[derive(Clone)]
pub struct MoveList {
    moves: [Move; MAX_MOVES],
    len: usize,
}

impl MoveList {
    /// An empty list.
    pub const fn new() -> MoveList {
        MoveList {
            moves: [Move(0); MAX_MOVES],
            len: 0,
        }
    }

    /// Adds a move.
    pub fn push(&mut self, mv: Move) {
        self.moves[self.len] = mv;
        self.len += 1;
    }
}

impl Default for MoveList {
    fn default() -> MoveList {
        MoveList::new()
    }
}

impl Deref for MoveList {
    type Target = [Move];

    fn deref(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}

impl fmt::Debug for MoveList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> IntoIterator for &'a MoveList {
    type Item = &'a Move;
    type IntoIter = std::slice::Iter<'a, Move>;

    fn into_iter(self) -> Self::IntoIter {
        self.moves[..self.len].iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_unpacks() {
        let kinds = [
            MoveKind::Quiet,
            MoveKind::DoublePawnPush,
            MoveKind::Castle,
            MoveKind::Capture,
            MoveKind::EnPassant,
            MoveKind::Promotion {
                piece: PieceType::Knight,
                capture: false,
            },
            MoveKind::Promotion {
                piece: PieceType::Queen,
                capture: true,
            },
        ];
        for kind in kinds {
            for (from, to) in [(Square::A1, Square::H8), (Square::E7, Square::E8)] {
                let mv = Move::new(from, to, kind);
                assert_eq!((mv.from(), mv.to(), mv.kind()), (from, to, kind));
            }
        }
    }

    #[test]
    fn uci_notation() {
        assert_eq!(
            Move::new(Square::E2, Square::E4, MoveKind::DoublePawnPush).to_string(),
            "e2e4"
        );
        let promo = MoveKind::Promotion {
            piece: PieceType::Knight,
            capture: true,
        };
        assert_eq!(
            Move::new(Square::E7, Square::F8, promo).to_string(),
            "e7f8n"
        );
        let castle = Move::new(Square::E1, Square::H1, MoveKind::Castle);
        assert_eq!(castle.to_uci(false), "e1g1");
        assert_eq!(castle.to_uci(true), "e1h1");
        let long = Move::new(Square::E8, Square::A8, MoveKind::Castle);
        assert_eq!(long.to_uci(false), "e8c8");
    }

    #[test]
    fn captures() {
        assert!(Move::new(Square::E4, Square::D5, MoveKind::Capture).is_capture());
        assert!(Move::new(Square::E5, Square::D6, MoveKind::EnPassant).is_capture());
        assert!(!Move::new(Square::E1, Square::H1, MoveKind::Castle).is_capture());
    }

    #[test]
    fn move_list() {
        let mut list = MoveList::new();
        assert!(list.is_empty());
        list.push(Move::new(Square::E2, Square::E4, MoveKind::DoublePawnPush));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].to_string(), "e2e4");
    }
}
