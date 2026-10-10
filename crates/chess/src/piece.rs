//! Colours, piece types and pieces.

use std::fmt;
use std::ops::Not;

/// A side: White or Black.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// White, who moves first.
    White,
    /// Black.
    Black,
}

impl Color {
    /// Both colours, White first.
    pub const ALL: [Color; 2] = [Color::White, Color::Black];

    /// 0 for White, 1 for Black: handy for indexing per-colour arrays.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The colour's FEN letter: `'w'` or `'b'`.
    pub const fn to_char(self) -> char {
        match self {
            Color::White => 'w',
            Color::Black => 'b',
        }
    }

    /// Parses a FEN side-to-move letter, `'w'` or `'b'`.
    pub fn from_char(c: char) -> Option<Color> {
        match c {
            'w' => Some(Color::White),
            'b' => Some(Color::Black),
            _ => None,
        }
    }
}

impl Not for Color {
    type Output = Color;

    /// The other colour.
    fn not(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Color::White => "White",
            Color::Black => "Black",
        })
    }
}

/// The kind of a piece, without its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PieceType {
    /// Pawn.
    Pawn,
    /// Knight.
    Knight,
    /// Bishop.
    Bishop,
    /// Rook.
    Rook,
    /// Queen.
    Queen,
    /// King.
    King,
}

impl PieceType {
    /// All piece types, from pawn to king.
    pub const ALL: [PieceType; 6] = [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
        PieceType::King,
    ];

    /// 0 (pawn) to 5 (king): handy for indexing per-type arrays.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The lowercase letter used in FEN and algebraic notation: `p n b r q k`.
    pub const fn to_char(self) -> char {
        match self {
            PieceType::Pawn => 'p',
            PieceType::Knight => 'n',
            PieceType::Bishop => 'b',
            PieceType::Rook => 'r',
            PieceType::Queen => 'q',
            PieceType::King => 'k',
        }
    }

    /// Parses a piece letter in either case.
    pub fn from_char(c: char) -> Option<PieceType> {
        match c.to_ascii_lowercase() {
            'p' => Some(PieceType::Pawn),
            'n' => Some(PieceType::Knight),
            'b' => Some(PieceType::Bishop),
            'r' => Some(PieceType::Rook),
            'q' => Some(PieceType::Queen),
            'k' => Some(PieceType::King),
            _ => None,
        }
    }
}

/// A piece of a given colour, like a white knight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Piece {
    /// Whose piece it is.
    pub color: Color,
    /// What kind of piece it is.
    pub piece_type: PieceType,
}

impl Piece {
    /// A piece of the given colour and type.
    pub const fn new(color: Color, piece_type: PieceType) -> Piece {
        Piece { color, piece_type }
    }

    /// The FEN letter: uppercase for White (`N`), lowercase for Black (`n`).
    pub const fn to_fen_char(self) -> char {
        let c = self.piece_type.to_char();
        match self.color {
            Color::White => c.to_ascii_uppercase(),
            Color::Black => c,
        }
    }

    /// Parses a FEN piece letter: uppercase is White, lowercase is Black.
    pub fn from_fen_char(c: char) -> Option<Piece> {
        let piece_type = PieceType::from_char(c)?;
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };
        Some(Piece::new(color, piece_type))
    }
}

impl fmt::Display for Piece {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_fen_char())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_flip() {
        assert_eq!(!Color::White, Color::Black);
        assert_eq!(!!Color::White, Color::White);
    }

    #[test]
    fn fen_letters_round_trip() {
        for color in Color::ALL {
            for piece_type in PieceType::ALL {
                let piece = Piece::new(color, piece_type);
                assert_eq!(Piece::from_fen_char(piece.to_fen_char()), Some(piece));
            }
        }
        assert_eq!(
            Piece::from_fen_char('N'),
            Some(Piece::new(Color::White, PieceType::Knight))
        );
        assert_eq!(
            Piece::from_fen_char('q'),
            Some(Piece::new(Color::Black, PieceType::Queen))
        );
        assert_eq!(Piece::from_fen_char('x'), None);
    }
}
