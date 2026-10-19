//! Standard Algebraic Notation (SAN): moves as people write them, like `Nf3`, `exd5`, `O-O` or
//! `e8=Q+`.
//!
//! ## Writing SAN
//!
//! - **Pieces** are a capital letter (`N B R Q K`) and the destination square: `Nf3`.
//! - **Pawns** are just the destination (`e4`), or the starting file, `x` and the destination
//!   for a capture (`exd5`).
//! - **Captures** add `x` before the destination: `Bxe5`.
//! - **Promotions** add `=` and the new piece: `e8=Q`.
//! - **Castling** is `O-O` (king side) or `O-O-O` (queen side), also in Chess960.
//! - **Disambiguation:** if two pieces of the same kind could reach the square, add the
//!   starting file (`Nbd7`), else the starting rank (`R1e2`), else both (`Qh4e1`).
//! - **Check** adds `+` and **checkmate** `#`.
//!
//! ## Reading SAN
//!
//! Reading is forgiving, because people and programs bend the rules: the check marks and
//! annotations (`+ # ! ?`) are ignored, `0-0` works like `O-O`, the `=` before a promotion
//! piece is optional, and unnecessary disambiguation is accepted. The text must still match
//! exactly one legal move. When it doesn't, the error says why, so a program can ask the
//! right question: it isn't shaped like a move, no legal move matches, more than one does,
//! or it is a pawn move to the last rank that leaves out the piece (`e8`, where `e8=Q` and
//! three others are legal).
//!
//! ## Reading either notation
//!
//! People type moves both ways: SAN (`Nf3`) and the coordinates UCI uses (`g1f3`, `e7e8q`).
//! [`Position::parse_move`] reads both, for programs that take a move from a person (the
//! shell, a GUI's chat). Text shaped like coordinates is read as coordinates, and anything
//! else as SAN. The order matters: `g1f3` is a fine UCI move, but as SAN it would be a pawn
//! move from g1, which doesn't exist. Coordinate-shaped text that is also SAN (`e2e4`, a
//! pawn's move with its starting square) means the same move either way, so nothing is lost.
//!
//! See the Chess Programming Wiki, [Algebraic Chess Notation](https://www.chessprogramming.org/Algebraic_Chess_Notation).

use std::fmt;

use crate::moves::{Move, MoveKind};
use crate::piece::PieceType;
use crate::position::Position;
use crate::square::{File, Rank, Square};

/// Why move text (SAN, or coordinates for [`Position::parse_move`]) doesn't name a legal
/// move. Each variant holds the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SanError {
    /// The text isn't shaped like a move.
    Syntax(String),
    /// No legal move matches.
    Illegal(String),
    /// More than one legal move matches; more disambiguation is needed.
    Ambiguous(String),
    /// A pawn move to the last rank that doesn't say which piece the pawn becomes (`e8`,
    /// `e7e8`). The same text with a piece (`e8=Q`, `e7e8q`) is legal.
    MissingPromotion(String),
}

impl fmt::Display for SanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SanError::Syntax(s) => write!(f, "'{s}' is not a move in algebraic notation"),
            SanError::Illegal(s) => write!(f, "'{s}' is not a legal move here"),
            SanError::Ambiguous(s) => write!(f, "'{s}' could mean more than one move"),
            SanError::MissingPromotion(s) => {
                write!(f, "'{s}' doesn't say which piece the pawn becomes")
            }
        }
    }
}

impl std::error::Error for SanError {}

/// The capital letter for a piece type in SAN (pawns have none).
fn piece_letter(piece_type: PieceType) -> char {
    piece_type.to_char().to_ascii_uppercase()
}

impl Position {
    // ANCHOR: to_san
    /// Writes `mv`, which must be legal here, in SAN.
    ///
    /// ```
    /// use cb2026_chess::Position;
    ///
    /// let pos = Position::startpos();
    /// let mv = pos.parse_uci_move("g1f3").unwrap();
    /// assert_eq!(pos.to_san(mv), "Nf3");
    /// ```
    pub fn to_san(&self, mv: Move) -> String {
        let mut san = self.san_without_check(mv);
        let mut after = self.clone();
        after.make_move(mv);
        if after.in_check() {
            san.push(if after.legal_moves().is_empty() {
                '#'
            } else {
                '+'
            });
        }
        san
    }
    // ANCHOR_END: to_san

    fn san_without_check(&self, mv: Move) -> String {
        let from = mv.from();
        let to = mv.to();
        if mv.kind() == MoveKind::Castle {
            return if to.file() > from.file() {
                "O-O".to_string()
            } else {
                "O-O-O".to_string()
            };
        }
        let piece_type = self
            .piece_at(from)
            .expect("a legal move starts on a piece")
            .piece_type;
        let mut san = String::new();
        if piece_type == PieceType::Pawn {
            if mv.is_capture() {
                san.push(from.file().to_char());
                san.push('x');
            }
            san.push_str(&to.to_string());
            if let Some(promotion) = mv.promotion() {
                san.push('=');
                san.push(piece_letter(promotion));
            }
            return san;
        }

        san.push(piece_letter(piece_type));
        // Other pieces of the same kind that could also move to `to`.
        let rivals: Vec<Square> = self
            .legal_moves()
            .iter()
            .filter(|other| {
                other.to() == to
                    && other.from() != from
                    && other.kind() != MoveKind::Castle
                    && self.piece_at(other.from()).map(|p| p.piece_type) == Some(piece_type)
            })
            .map(|other| other.from())
            .collect();
        if !rivals.is_empty() {
            if rivals.iter().all(|r| r.file() != from.file()) {
                san.push(from.file().to_char());
            } else if rivals.iter().all(|r| r.rank() != from.rank()) {
                san.push(from.rank().to_char());
            } else {
                san.push_str(&from.to_string());
            }
        }
        if mv.is_capture() {
            san.push('x');
        }
        san.push_str(&to.to_string());
        san
    }

    /// Finds the legal move written as `san`.
    ///
    /// ```
    /// use cb2026_chess::Position;
    ///
    /// let pos = Position::startpos();
    /// assert_eq!(pos.parse_san("Nf3").unwrap().to_string(), "g1f3");
    /// assert_eq!(pos.parse_san("e4").unwrap().to_string(), "e2e4");
    /// assert!(pos.parse_san("e5").is_err());
    /// ```
    pub fn parse_san(&self, san: &str) -> Result<Move, SanError> {
        let text: String = san
            .trim()
            .trim_end_matches(['+', '#', '!', '?'])
            .replace('0', "O");
        let syntax = || SanError::Syntax(san.to_string());
        let candidates = self.legal_moves();

        if text == "O-O" || text == "O-O-O" {
            let king_side = text == "O-O";
            return candidates
                .iter()
                .copied()
                .find(|mv| {
                    mv.kind() == MoveKind::Castle
                        && (mv.to().file() > mv.from().file()) == king_side
                })
                .ok_or_else(|| SanError::Illegal(san.to_string()));
        }

        let mut chars: Vec<char> = text.chars().filter(|&c| c != 'x' && c != '=').collect();
        // A leading capital letter names the piece; otherwise it is a pawn move.
        let piece_type = match chars.first() {
            Some(&c) if "NBRQK".contains(c) => {
                chars.remove(0);
                PieceType::from_char(c).expect("piece letter")
            }
            _ => PieceType::Pawn,
        };
        // A trailing piece letter is a promotion.
        let promotion = match chars.last() {
            Some(&c) if "NBRQnbrq".contains(c) && piece_type == PieceType::Pawn => {
                chars.pop();
                Some(PieceType::from_char(c).expect("piece letter"))
            }
            _ => None,
        };
        if chars.len() < 2 || chars.len() > 4 {
            return Err(syntax());
        }
        let destination: String = chars[chars.len() - 2..].iter().collect();
        let to: Square = destination.parse().map_err(|_| syntax())?;
        // Whatever is left between piece and destination narrows down the starting square.
        let mut from_file: Option<File> = None;
        let mut from_rank: Option<Rank> = None;
        for &c in &chars[..chars.len() - 2] {
            if let Some(file) = File::from_char(c) {
                from_file = Some(file);
            } else if let Some(rank) = Rank::from_char(c) {
                from_rank = Some(rank);
            } else {
                return Err(syntax());
            }
        }

        // The moves the text could mean, whatever they promote to, then those that promote
        // to what it says (nothing, for a move that doesn't promote).
        let on_squares: Vec<Move> = candidates
            .iter()
            .copied()
            .filter(|mv| {
                mv.kind() != MoveKind::Castle
                    && mv.to() == to
                    && self.piece_at(mv.from()).map(|p| p.piece_type) == Some(piece_type)
                    && from_file.is_none_or(|f| mv.from().file() == f)
                    && from_rank.is_none_or(|r| mv.from().rank() == r)
            })
            .collect();
        let matches: Vec<&Move> = on_squares
            .iter()
            .filter(|mv| mv.promotion() == promotion)
            .collect();
        match matches.as_slice() {
            [mv] => Ok(**mv),
            [_, _, ..] => Err(SanError::Ambiguous(san.to_string())),
            // No piece named, and only promotions reach the square: one pawn's four, or
            // several pawns' (which is ambiguous before the piece even matters).
            [] if !on_squares.is_empty() && promotion.is_none() => {
                if on_squares
                    .iter()
                    .all(|mv| mv.from() == on_squares[0].from())
                {
                    Err(SanError::MissingPromotion(san.to_string()))
                } else {
                    Err(SanError::Ambiguous(san.to_string()))
                }
            }
            [] => Err(SanError::Illegal(san.to_string())),
        }
    }

    /// Finds the legal move written as `text` in either notation (see [Reading either
    /// notation](self)): UCI's coordinates (`g1f3`, `e7e8q`, castling as `e1g1` or as the king
    /// taking its own rook, `e1h1`) or SAN (`Nf3`, `O-O`, `e8=Q`). Unlike
    /// [`Position::parse_uci_move`], the error says why the text names no legal move, and
    /// both notations report a promotion without its piece as
    /// [`SanError::MissingPromotion`].
    ///
    /// ```
    /// use cb2026_chess::{Position, SanError};
    ///
    /// let pos = Position::startpos();
    /// assert_eq!(pos.parse_move("g1f3").unwrap().to_string(), "g1f3");
    /// assert_eq!(pos.parse_move("Nf3").unwrap().to_string(), "g1f3");
    /// assert_eq!(pos.parse_move("e2e5"), Err(SanError::Illegal("e2e5".to_string())));
    ///
    /// let pawn = Position::from_fen("8/4P3/8/8/8/8/k7/4K3 w - - 0 1").unwrap();
    /// assert_eq!(pawn.parse_move("e7e8"), Err(SanError::MissingPromotion("e7e8".to_string())));
    /// assert_eq!(pawn.parse_move("e8"), Err(SanError::MissingPromotion("e8".to_string())));
    /// assert_eq!(pawn.parse_move("e7e8q").unwrap().to_string(), "e7e8q");
    /// ```
    pub fn parse_move(&self, text: &str) -> Result<Move, SanError> {
        let trimmed = text.trim();
        let Some((from, to, piece)) = coordinates(trimmed) else {
            return self.parse_san(text);
        };
        if let Some(mv) = self.parse_uci_move(trimmed) {
            return Ok(mv);
        }
        let promotes = self
            .legal_moves()
            .iter()
            .any(|mv| mv.from() == from && mv.to() == to && mv.promotion().is_some());
        if promotes && piece.is_none() {
            Err(SanError::MissingPromotion(text.to_string()))
        } else {
            Err(SanError::Illegal(text.to_string()))
        }
    }
}

/// The squares and promotion letter of text shaped like UCI's coordinates: two squares, and
/// a lower-case piece letter for a promotion (`e2e4`, `e7e8q`). `None` for anything else.
fn coordinates(text: &str) -> Option<(Square, Square, Option<char>)> {
    if !text.is_ascii() {
        return None;
    }
    let piece = match text.len() {
        4 => None,
        5 => Some(text.chars().last().filter(|c| "nbrq".contains(*c))?),
        _ => return None,
    };
    let from = text[..2].parse().ok()?;
    let to = text[2..4].parse().ok()?;
    Some((from, to, piece))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn san_of(fen: &str, uci: &str) -> String {
        let pos = Position::from_fen(fen).unwrap();
        pos.to_san(pos.parse_uci_move(uci).unwrap())
    }

    #[test]
    fn writes_san() {
        let start = crate::STARTING_FEN;
        assert_eq!(san_of(start, "e2e4"), "e4");
        assert_eq!(san_of(start, "b1c3"), "Nc3");
        let kiwipete = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        assert_eq!(san_of(kiwipete, "e1g1"), "O-O");
        assert_eq!(san_of(kiwipete, "e1c1"), "O-O-O");
        assert_eq!(san_of(kiwipete, "d5e6"), "dxe6");
        assert_eq!(san_of(kiwipete, "e5f7"), "Nxf7");
        assert_eq!(san_of(kiwipete, "f3f6"), "Qxf6");
    }

    #[test]
    fn disambiguates() {
        // Knights on b1 and f3 can both reach d2: by file.
        assert_eq!(san_of("4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1", "b1d2"), "Nbd2");
        // Rooks on a1 and a5 can both reach a3: by rank.
        assert_eq!(san_of("4k3/8/8/R7/8/8/8/R3K3 w - - 0 1", "a1a3"), "R1a3");
        // Queens on h4, e4 and h1 can all reach e1: the h4 queen needs file and rank.
        assert_eq!(san_of("2k5/8/8/8/4Q2Q/8/8/K6Q w - - 0 1", "h4e1"), "Qh4e1");
    }

    #[test]
    fn promotions_checks_and_mate() {
        assert_eq!(san_of("8/P6k/8/8/8/8/8/K7 w - - 0 1", "a7a8q"), "a8=Q");
        assert_eq!(san_of("7k/P7/8/8/8/8/8/K7 w - - 0 1", "a7a8q"), "a8=Q+");
        let fools_mate = "rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq g3 0 2";
        assert_eq!(san_of(fools_mate, "d8h4"), "Qh4#");
    }

    #[test]
    fn san_round_trips_for_every_legal_move() {
        for case in crate::perft::parse_suite(include_str!("../../../tests/data/perft.epd")) {
            let pos = Position::from_fen(&case.fen).unwrap();
            for &mv in pos.legal_moves().iter() {
                let san = pos.to_san(mv);
                assert_eq!(pos.parse_san(&san), Ok(mv), "{san} in {}", case.fen);
            }
        }
    }

    #[test]
    fn reads_loose_san() {
        let pos = Position::startpos();
        assert_eq!(pos.parse_san("Ng1f3").unwrap().to_string(), "g1f3");
        assert_eq!(pos.parse_san("Nf3+!?").unwrap().to_string(), "g1f3");
        let pos = Position::from_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
        assert_eq!(pos.parse_san("a8Q").unwrap().to_string(), "a7a8q");
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        assert_eq!(pos.parse_san("0-0").unwrap().to_string(), "e1g1");
    }

    #[test]
    fn rejects_bad_san() {
        let pos = Position::startpos();
        assert!(matches!(pos.parse_san("Nf4"), Err(SanError::Illegal(_))));
        assert!(matches!(pos.parse_san("hello"), Err(SanError::Syntax(_))));
        let two_knights = Position::from_fen("4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1").unwrap();
        assert!(matches!(
            two_knights.parse_san("Nd2"),
            Err(SanError::Ambiguous(_))
        ));
    }

    /// A white pawn on e7 with e8 free, and the kings out of the way.
    const PAWN_ON_E7: &str = "8/4P3/8/8/8/8/k7/4K3 w - - 0 1";

    fn missing(text: &str) -> Result<Move, SanError> {
        Err(SanError::MissingPromotion(text.to_string()))
    }

    #[test]
    fn a_promotion_without_its_piece_says_so() {
        let pawn = Position::from_fen(PAWN_ON_E7).unwrap();
        assert_eq!(pawn.parse_san("e8"), missing("e8"));
        assert_eq!(pawn.parse_san("e8+"), missing("e8+"));
        assert_eq!(pawn.parse_san("e8=N").unwrap().to_string(), "e7e8n");
        // Two pawns can take on e8: which pawn matters before which piece.
        let two = Position::from_fen("4r3/3P1P2/8/8/8/8/k7/4K3 w - - 0 1").unwrap();
        assert!(matches!(two.parse_san("e8"), Err(SanError::Ambiguous(_))));
        assert_eq!(two.parse_san("dxe8"), missing("dxe8"));
        assert_eq!(two.parse_san("dxe8=Q").unwrap().to_string(), "d7e8q");
    }

    #[test]
    fn reads_moves_in_either_notation() {
        let pos = Position::startpos();
        for (text, uci) in [
            ("e4", "e2e4"),
            ("e2e4", "e2e4"),
            ("Nf3", "g1f3"),
            // As SAN this would be a pawn on g1: coordinates are read first.
            ("g1f3", "g1f3"),
            (" b1c3 ", "b1c3"),
        ] {
            assert_eq!(pos.parse_move(text).unwrap().to_string(), uci, "{text}");
        }
        let castling = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        for text in ["e1g1", "e1h1", "O-O", "0-0"] {
            assert_eq!(
                castling.parse_move(text).unwrap().to_string(),
                "e1g1",
                "{text}"
            );
        }
        let pawn = Position::from_fen(PAWN_ON_E7).unwrap();
        assert_eq!(pawn.parse_move("e7e8q").unwrap().to_string(), "e7e8q");
        assert_eq!(pawn.parse_move("e8=Q").unwrap().to_string(), "e7e8q");
    }

    #[test]
    fn says_why_a_move_in_either_notation_names_none() {
        let pos = Position::startpos();
        let illegal = |text: &str| Err(SanError::Illegal(text.to_string()));
        assert_eq!(pos.parse_move("e2e5"), illegal("e2e5"));
        assert_eq!(pos.parse_move("e2e4q"), illegal("e2e4q"));
        assert_eq!(pos.parse_move("Nf4"), illegal("Nf4"));
        assert!(matches!(pos.parse_move("hello"), Err(SanError::Syntax(_))));
        // Not a promotion letter, so not coordinates; and not SAN either.
        assert!(matches!(pos.parse_move("e2e4k"), Err(SanError::Syntax(_))));
        // Text that isn't ASCII is never coordinates (and slicing it as if it were would
        // panic in the middle of a character).
        assert!(matches!(pos.parse_move("é2e4"), Err(SanError::Syntax(_))));
        let pawn = Position::from_fen(PAWN_ON_E7).unwrap();
        assert_eq!(pawn.parse_move("e7e8"), missing("e7e8"));
        assert_eq!(pawn.parse_move("e8"), missing("e8"));
    }

    #[test]
    fn every_legal_move_reads_back_in_either_notation() {
        for case in crate::perft::parse_suite(include_str!("../../../tests/data/perft.epd")) {
            let pos = Position::from_fen(&case.fen).unwrap();
            for &mv in pos.legal_moves().iter() {
                let san = pos.to_san(mv);
                let uci = mv.to_uci(false);
                assert_eq!(pos.parse_move(&san), Ok(mv), "{san} in {}", case.fen);
                assert_eq!(pos.parse_move(&uci), Ok(mv), "{uci} in {}", case.fen);
            }
        }
    }
}
