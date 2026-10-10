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
//! exactly one legal move.
//!
//! See the Chess Programming Wiki, [Algebraic Chess Notation](https://www.chessprogramming.org/Algebraic_Chess_Notation).

use std::fmt;

use crate::moves::{Move, MoveKind};
use crate::piece::PieceType;
use crate::position::Position;
use crate::square::{File, Rank, Square};

/// Why a SAN string doesn't name a legal move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SanError {
    /// The text isn't shaped like a move.
    Syntax(String),
    /// No legal move matches.
    Illegal(String),
    /// More than one legal move matches; more disambiguation is needed.
    Ambiguous(String),
}

impl fmt::Display for SanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SanError::Syntax(s) => write!(f, "'{s}' is not a move in algebraic notation"),
            SanError::Illegal(s) => write!(f, "'{s}' is not a legal move here"),
            SanError::Ambiguous(s) => write!(f, "'{s}' could mean more than one move"),
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

        let matches: Vec<Move> = candidates
            .iter()
            .copied()
            .filter(|mv| {
                mv.kind() != MoveKind::Castle
                    && mv.to() == to
                    && self.piece_at(mv.from()).map(|p| p.piece_type) == Some(piece_type)
                    && from_file.is_none_or(|f| mv.from().file() == f)
                    && from_rank.is_none_or(|r| mv.from().rank() == r)
                    && mv.promotion() == promotion
            })
            .collect();
        match matches.as_slice() {
            [mv] => Ok(*mv),
            [] => Err(SanError::Illegal(san.to_string())),
            _ => Err(SanError::Ambiguous(san.to_string())),
        }
    }
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
}
