//! Squares, files and ranks.
//!
//! ## Numbering the squares
//!
//! The 64 squares are numbered 0 to 63, starting at a1 and going along each rank:
//!
//! ```text
//!  8 | 56 57 58 59 60 61 62 63
//!  7 | 48 49 50 51 52 53 54 55
//!  6 | 40 41 42 43 44 45 46 47
//!  5 | 32 33 34 35 36 37 38 39
//!  4 | 24 25 26 27 28 29 30 31
//!  3 | 16 17 18 19 20 21 22 23
//!  2 |  8  9 10 11 12 13 14 15
//!  1 |  0  1  2  3  4  5  6  7
//!    +------------------------
//!       a  b  c  d  e  f  g  h
//! ```
//!
//! This is the "little-endian rank-file" mapping used by most bitboard engines. It makes the
//! arithmetic simple: `index = rank * 8 + file`, one step north is `+8`, one step east is `+1`,
//! and square `i` is bit `i` of a [`Bitboard`](crate::Bitboard).
//!
//! See the Chess Programming Wiki, [Square Mapping Considerations](https://www.chessprogramming.org/Square_Mapping_Considerations).

use std::fmt;
use std::str::FromStr;

/// A file (column) of the board, a to h.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum File {
    /// The a-file.
    A,
    /// The b-file.
    B,
    /// The c-file.
    C,
    /// The d-file.
    D,
    /// The e-file.
    E,
    /// The f-file.
    F,
    /// The g-file.
    G,
    /// The h-file.
    H,
}

impl File {
    /// All files, from a to h.
    pub const ALL: [File; 8] = [
        File::A,
        File::B,
        File::C,
        File::D,
        File::E,
        File::F,
        File::G,
        File::H,
    ];

    /// The file's index, 0 (a) to 7 (h).
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The file with the given index, if it is in 0..8.
    pub const fn from_index(index: usize) -> Option<File> {
        if index < 8 {
            Some(File::ALL[index])
        } else {
            None
        }
    }

    /// Parses a lowercase file letter, `'a'` to `'h'`.
    pub fn from_char(c: char) -> Option<File> {
        match c {
            'a'..='h' => File::from_index(c as usize - 'a' as usize),
            _ => None,
        }
    }

    /// The file's lowercase letter, `'a'` to `'h'`.
    pub const fn to_char(self) -> char {
        (b'a' + self as u8) as char
    }
}

/// A rank (row) of the board, 1 to 8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rank {
    /// Rank 1, where White's pieces start.
    First,
    /// Rank 2, where White's pawns start.
    Second,
    /// Rank 3.
    Third,
    /// Rank 4.
    Fourth,
    /// Rank 5.
    Fifth,
    /// Rank 6.
    Sixth,
    /// Rank 7, where Black's pawns start.
    Seventh,
    /// Rank 8, where Black's pieces start.
    Eighth,
}

impl Rank {
    /// All ranks, from 1 to 8.
    pub const ALL: [Rank; 8] = [
        Rank::First,
        Rank::Second,
        Rank::Third,
        Rank::Fourth,
        Rank::Fifth,
        Rank::Sixth,
        Rank::Seventh,
        Rank::Eighth,
    ];

    /// The rank's index, 0 (rank 1) to 7 (rank 8).
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The rank with the given index, if it is in 0..8.
    pub const fn from_index(index: usize) -> Option<Rank> {
        if index < 8 {
            Some(Rank::ALL[index])
        } else {
            None
        }
    }

    /// Parses a rank digit, `'1'` to `'8'`.
    pub fn from_char(c: char) -> Option<Rank> {
        match c {
            '1'..='8' => Rank::from_index(c as usize - '1' as usize),
            _ => None,
        }
    }

    /// The rank's digit, `'1'` to `'8'`.
    pub const fn to_char(self) -> char {
        (b'1' + self as u8) as char
    }
}

// ANCHOR: square
/// One of the 64 squares, numbered as described in the [module docs](self).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Square(u8);

impl Square {
    /// The square on the given file and rank.
    pub const fn new(file: File, rank: Rank) -> Square {
        Square((rank as u8) * 8 + file as u8)
    }
    // ANCHOR_END: square

    /// The square with the given index, if it is in 0..64.
    pub const fn from_index(index: usize) -> Option<Square> {
        if index < 64 {
            Some(Square(index as u8))
        } else {
            None
        }
    }

    /// The square's index, 0 (a1) to 63 (h8).
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The square's file.
    pub const fn file(self) -> File {
        File::ALL[(self.0 % 8) as usize]
    }

    /// The square's rank.
    pub const fn rank(self) -> Rank {
        Rank::ALL[(self.0 / 8) as usize]
    }

    /// The same file on the opposite side of the board: a1 becomes a8, e4 becomes e5.
    ///
    /// Useful for seeing the board from Black's side and for writing evaluation tables once
    /// and using them for both colours.
    pub const fn flip_rank(self) -> Square {
        Square(self.0 ^ 56)
    }

    /// All 64 squares, from a1 to h8.
    pub fn all() -> impl DoubleEndedIterator<Item = Square> {
        (0..64).map(Square)
    }
}

/// Error returned when a string is not a square name like `e4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseSquareError(pub String);

impl fmt::Display for ParseSquareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "'{}' is not a square (expected a file a-h and a rank 1-8, like e4)",
            self.0
        )
    }
}

impl std::error::Error for ParseSquareError {}

impl FromStr for Square {
    type Err = ParseSquareError;

    fn from_str(s: &str) -> Result<Square, ParseSquareError> {
        let mut chars = s.chars();
        match (chars.next(), chars.next(), chars.next()) {
            (Some(f), Some(r), None) => match (File::from_char(f), Rank::from_char(r)) {
                (Some(file), Some(rank)) => Ok(Square::new(file, rank)),
                _ => Err(ParseSquareError(s.to_string())),
            },
            _ => Err(ParseSquareError(s.to_string())),
        }
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.file().to_char(), self.rank().to_char())
    }
}

impl fmt::Debug for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Defines a named constant for every square (`Square::A1` ... `Square::H8`).
macro_rules! square_constants {
    ($($name:ident = $index:expr),* $(,)?) => {
        impl Square {
            $(
                #[doc = concat!("The ", stringify!($name), " square.")]
                pub const $name: Square = Square($index);
            )*
        }
    };
}

square_constants! {
    A1 = 0,
    B1 = 1,
    C1 = 2,
    D1 = 3,
    E1 = 4,
    F1 = 5,
    G1 = 6,
    H1 = 7,
    A2 = 8,
    B2 = 9,
    C2 = 10,
    D2 = 11,
    E2 = 12,
    F2 = 13,
    G2 = 14,
    H2 = 15,
    A3 = 16,
    B3 = 17,
    C3 = 18,
    D3 = 19,
    E3 = 20,
    F3 = 21,
    G3 = 22,
    H3 = 23,
    A4 = 24,
    B4 = 25,
    C4 = 26,
    D4 = 27,
    E4 = 28,
    F4 = 29,
    G4 = 30,
    H4 = 31,
    A5 = 32,
    B5 = 33,
    C5 = 34,
    D5 = 35,
    E5 = 36,
    F5 = 37,
    G5 = 38,
    H5 = 39,
    A6 = 40,
    B6 = 41,
    C6 = 42,
    D6 = 43,
    E6 = 44,
    F6 = 45,
    G6 = 46,
    H6 = 47,
    A7 = 48,
    B7 = 49,
    C7 = 50,
    D7 = 51,
    E7 = 52,
    F7 = 53,
    G7 = 54,
    H7 = 55,
    A8 = 56,
    B8 = 57,
    C8 = 58,
    D8 = 59,
    E8 = 60,
    F8 = 61,
    G8 = 62,
    H8 = 63
,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_file_and_rank_agree() {
        for square in Square::all() {
            assert_eq!(
                square.index(),
                square.rank().index() * 8 + square.file().index()
            );
            assert_eq!(Square::new(square.file(), square.rank()), square);
        }
    }

    #[test]
    fn named_constants() {
        assert_eq!(Square::A1.index(), 0);
        assert_eq!(Square::H1.index(), 7);
        assert_eq!(Square::E4.index(), 28);
        assert_eq!(Square::H8.index(), 63);
    }

    #[test]
    fn parse_and_display_round_trip() {
        for square in Square::all() {
            let name = square.to_string();
            assert_eq!(name.parse::<Square>(), Ok(square));
        }
        assert_eq!("e4".parse::<Square>(), Ok(Square::E4));
    }

    #[test]
    fn rejects_bad_names() {
        for bad in ["", "e", "e9", "i1", "E4", "e44", "4e"] {
            assert!(bad.parse::<Square>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn flip_rank_mirrors_vertically() {
        assert_eq!(Square::A1.flip_rank(), Square::A8);
        assert_eq!(Square::E4.flip_rank(), Square::E5);
        for square in Square::all() {
            assert_eq!(square.flip_rank().flip_rank(), square);
            assert_eq!(square.flip_rank().file(), square.file());
        }
    }
}
