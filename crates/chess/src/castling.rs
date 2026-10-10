//! Castling rights.
//!
//! In standard chess the castling rooks always start on the a- and h-files, so engines often
//! store castling rights as four flags (`KQkq`). In Chess960 the rooks can start on any file,
//! so cb2026 stores *which rook* each right refers to: the file of the rook on the back rank.
//! Standard chess is then just the case where those files are a and h, and the same code
//! handles both games.
//!
//! See the Chess Programming Wiki, [Castling Rights](https://www.chessprogramming.org/Castling_Rights)
//! and [Chess960](https://www.chessprogramming.org/Chess960).

use crate::piece::Color;
use crate::square::File;

/// Which side of the king a castling move goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastlingSide {
    /// Towards the h-file ("short" castling, O-O).
    KingSide,
    /// Towards the a-file ("long" castling, O-O-O).
    QueenSide,
}

impl CastlingSide {
    /// Both sides, king side first.
    pub const ALL: [CastlingSide; 2] = [CastlingSide::KingSide, CastlingSide::QueenSide];

    /// 0 for the king side, 1 for the queen side.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Which castling moves are still allowed, stored as the file of each castling rook.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CastlingRights {
    /// `rooks[color][side]` is the file of the rook that colour may still castle with on that
    /// side, or `None` if that right is gone.
    rooks: [[Option<File>; 2]; 2],
}

impl CastlingRights {
    /// No castling rights at all.
    pub const NONE: CastlingRights = CastlingRights {
        rooks: [[None; 2]; 2],
    };

    /// The rights at the start of a standard game: both sides may castle both ways, with the
    /// rooks on the a- and h-files.
    pub const STANDARD: CastlingRights = CastlingRights {
        rooks: [
            [Some(File::H), Some(File::A)],
            [Some(File::H), Some(File::A)],
        ],
    };

    /// The file of the rook `color` may castle with on `side`, if that right still exists.
    pub const fn rook_file(self, color: Color, side: CastlingSide) -> Option<File> {
        self.rooks[color.index()][side.index()]
    }

    /// Whether `color` may still castle on `side`.
    pub const fn has(self, color: Color, side: CastlingSide) -> bool {
        self.rook_file(color, side).is_some()
    }

    /// Whether neither side has any castling rights left.
    pub fn is_empty(self) -> bool {
        self == CastlingRights::NONE
    }

    /// Gives `color` the right to castle on `side` with the rook on `rook_file`.
    pub fn set(&mut self, color: Color, side: CastlingSide, rook_file: File) {
        self.rooks[color.index()][side.index()] = Some(rook_file);
    }

    /// Removes `color`'s right to castle on `side`.
    pub fn remove(&mut self, color: Color, side: CastlingSide) {
        self.rooks[color.index()][side.index()] = None;
    }

    /// Removes both of `color`'s castling rights (for example when its king moves).
    pub fn remove_color(&mut self, color: Color) {
        self.rooks[color.index()] = [None; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_rights() {
        let rights = CastlingRights::STANDARD;
        for color in Color::ALL {
            assert_eq!(
                rights.rook_file(color, CastlingSide::KingSide),
                Some(File::H)
            );
            assert_eq!(
                rights.rook_file(color, CastlingSide::QueenSide),
                Some(File::A)
            );
        }
    }

    #[test]
    fn set_and_remove() {
        let mut rights = CastlingRights::NONE;
        assert!(rights.is_empty());
        rights.set(Color::White, CastlingSide::KingSide, File::G);
        assert!(rights.has(Color::White, CastlingSide::KingSide));
        assert!(!rights.has(Color::Black, CastlingSide::KingSide));
        rights.remove(Color::White, CastlingSide::KingSide);
        assert!(rights.is_empty());

        let mut rights = CastlingRights::STANDARD;
        rights.remove_color(Color::Black);
        assert!(rights.has(Color::White, CastlingSide::QueenSide));
        assert!(!rights.has(Color::Black, CastlingSide::QueenSide));
    }
}
