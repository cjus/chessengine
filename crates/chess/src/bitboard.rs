//! Bitboards: sets of squares stored in a single 64-bit integer.
//!
//! A bitboard has one bit per square (bit `i` is square `i`, see [`crate::square`]). A set bit
//! means "this square is in the set". One bitboard can hold "all white pawns", "every square a
//! knight on g1 attacks" or "all empty squares", and set operations become single CPU
//! instructions:
//!
//! | Set operation        | Bitboard operation |
//! | -------------------- | ------------------ |
//! | union                | `a \| b`           |
//! | intersection         | `a & b`            |
//! | difference           | `a & !b`           |
//! | number of squares    | `a.count()` (one `popcnt` instruction) |
//! | first square         | `a.first()` (one "count trailing zeros" instruction) |
//!
//! That is why almost every strong engine uses bitboards: the questions move generation and
//! evaluation ask ("which of my pieces attack the enemy king zone?") become a handful of
//! instructions instead of loops over the board.
//!
//! See the Chess Programming Wiki, [Bitboards](https://www.chessprogramming.org/Bitboards).

use std::fmt;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

use crate::square::{File, Rank, Square};

// ANCHOR: bitboard
/// A set of squares, one bit per square.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Bitboard(pub u64);

impl Bitboard {
    /// The empty set.
    pub const EMPTY: Bitboard = Bitboard(0);
    /// All 64 squares.
    pub const ALL: Bitboard = Bitboard(!0);

    /// The set containing just `square`.
    pub const fn from_square(square: Square) -> Bitboard {
        Bitboard(1 << square.index())
    }

    /// Whether `square` is in the set.
    pub const fn contains(self, square: Square) -> bool {
        self.0 & (1 << square.index()) != 0
    }
    // ANCHOR_END: bitboard

    /// All squares on `file`.
    pub const fn file(file: File) -> Bitboard {
        Bitboard(0x0101_0101_0101_0101 << file.index())
    }

    /// All squares on `rank`.
    pub const fn rank(rank: Rank) -> Bitboard {
        Bitboard(0xff << (8 * rank.index()))
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The number of squares in the set.
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }

    /// The set with `square` added.
    #[must_use]
    pub const fn with(self, square: Square) -> Bitboard {
        Bitboard(self.0 | (1 << square.index()))
    }

    /// The set with `square` removed.
    #[must_use]
    pub const fn without(self, square: Square) -> Bitboard {
        Bitboard(self.0 & !(1 << square.index()))
    }

    /// The lowest-numbered square in the set (closest to a1), if any.
    pub const fn first(self) -> Option<Square> {
        if self.0 == 0 {
            None
        } else {
            Square::from_index(self.0.trailing_zeros() as usize)
        }
    }

    /// The squares in the set, from a1 towards h8.
    pub const fn squares(self) -> Squares {
        Squares(self)
    }
}

/// Iterator over the squares of a [`Bitboard`], lowest first.
///
/// Each step finds the lowest set bit and clears it, so it costs one iteration per square in
/// the set, not one per square on the board.
#[derive(Clone, Debug)]
pub struct Squares(Bitboard);

impl Iterator for Squares {
    type Item = Square;

    fn next(&mut self) -> Option<Square> {
        let square = self.0.first()?;
        // Clear the lowest set bit: `x & (x - 1)` turns it off and leaves the others alone.
        self.0.0 &= self.0.0 - 1;
        Some(square)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.0.count() as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Squares {}

impl IntoIterator for Bitboard {
    type Item = Square;
    type IntoIter = Squares;

    fn into_iter(self) -> Squares {
        self.squares()
    }
}

impl FromIterator<Square> for Bitboard {
    fn from_iter<I: IntoIterator<Item = Square>>(iter: I) -> Bitboard {
        iter.into_iter().fold(Bitboard::EMPTY, Bitboard::with)
    }
}

impl From<Square> for Bitboard {
    fn from(square: Square) -> Bitboard {
        Bitboard::from_square(square)
    }
}

impl BitAnd for Bitboard {
    type Output = Bitboard;
    fn bitand(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 & rhs.0)
    }
}

impl BitOr for Bitboard {
    type Output = Bitboard;
    fn bitor(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 | rhs.0)
    }
}

impl BitXor for Bitboard {
    type Output = Bitboard;
    fn bitxor(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 ^ rhs.0)
    }
}

impl Not for Bitboard {
    type Output = Bitboard;
    fn not(self) -> Bitboard {
        Bitboard(!self.0)
    }
}

impl BitAndAssign for Bitboard {
    fn bitand_assign(&mut self, rhs: Bitboard) {
        self.0 &= rhs.0;
    }
}

impl BitOrAssign for Bitboard {
    fn bitor_assign(&mut self, rhs: Bitboard) {
        self.0 |= rhs.0;
    }
}

impl BitXorAssign for Bitboard {
    fn bitxor_assign(&mut self, rhs: Bitboard) {
        self.0 ^= rhs.0;
    }
}

impl fmt::Debug for Bitboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bitboard({:#018x})", self.0)
    }
}

impl fmt::Display for Bitboard {
    /// Draws the set as an 8×8 grid, rank 8 at the top: `1` for a member, `.` otherwise.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in Rank::ALL.iter().rev() {
            for file in File::ALL {
                let mark = if self.contains(Square::new(file, *rank)) {
                    '1'
                } else {
                    '.'
                };
                write!(f, "{mark}")?;
                if file != File::H {
                    write!(f, " ")?;
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_membership() {
        let bb = Bitboard::EMPTY.with(Square::E4).with(Square::A1);
        assert!(bb.contains(Square::E4));
        assert!(bb.contains(Square::A1));
        assert!(!bb.contains(Square::E5));
        assert_eq!(bb.count(), 2);
        assert_eq!(bb.without(Square::E4), Bitboard::from_square(Square::A1));
    }

    #[test]
    fn files_and_ranks() {
        assert_eq!(Bitboard::rank(Rank::Second).0, 0xff00);
        assert_eq!(Bitboard::file(File::A).0, 0x0101_0101_0101_0101);
        assert_eq!(Bitboard::file(File::H).0, 0x8080_8080_8080_8080);
        for file in File::ALL {
            assert_eq!(Bitboard::file(file).count(), 8);
            for rank in Rank::ALL {
                let both = Bitboard::file(file) & Bitboard::rank(rank);
                assert_eq!(both, Bitboard::from_square(Square::new(file, rank)));
            }
        }
    }

    #[test]
    fn iterates_squares_in_order() {
        let bb: Bitboard = [Square::H8, Square::A1, Square::E4].into_iter().collect();
        let squares: Vec<Square> = bb.squares().collect();
        assert_eq!(squares, vec![Square::A1, Square::E4, Square::H8]);
        assert_eq!(bb.squares().len(), 3);
        assert_eq!(Bitboard::EMPTY.squares().next(), None);
        assert_eq!(Bitboard::ALL.squares().count(), 64);
    }

    #[test]
    fn set_operations() {
        let a = Bitboard::rank(Rank::First);
        let b = Bitboard::file(File::A);
        assert_eq!((a | b).count(), 15);
        assert_eq!((a & b), Bitboard::from_square(Square::A1));
        assert_eq!((a & !b).count(), 7);
        assert_eq!((a ^ b).count(), 14);
        assert_eq!(!Bitboard::EMPTY, Bitboard::ALL);
    }

    #[test]
    fn displays_as_a_grid() {
        let bb = Bitboard::from_square(Square::A1) | Bitboard::from_square(Square::H8);
        let expected = "\
. . . . . . . 1
. . . . . . . .
. . . . . . . .
. . . . . . . .
. . . . . . . .
. . . . . . . .
. . . . . . . .
1 . . . . . . .
";
        assert_eq!(bb.to_string(), expected);
    }
}
