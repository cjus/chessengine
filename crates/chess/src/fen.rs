//! Forsyth–Edwards Notation (FEN): a position written as one line of text.
//!
//! The starting position in FEN is:
//!
//! ```text
//! rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1
//! ```
//!
//! It has six fields separated by spaces:
//!
//! 1. **Piece placement**, rank 8 first, ranks separated by `/`. Each rank lists its squares
//!    from a to h: a letter for a piece (uppercase White, lowercase Black: `PNBRQK`) or a
//!    digit for that many empty squares.
//! 2. **Side to move**: `w` or `b`.
//! 3. **Castling rights**: some of `KQkq`, or `-` for none (see below for Chess960).
//! 4. **En passant square**: the square a pawn just skipped with a double step, or `-`.
//! 5. **Halfmove clock**: halfmoves since the last capture or pawn move.
//! 6. **Fullmove number**: starts at 1, increases after each Black move.
//!
//! The last two fields are optional here (they default to `0 1`), because test suites in EPD
//! format often leave them out.
//!
//! ## Castling in Chess960
//!
//! When the rooks don't start on a and h, `K` and `Q` can be ambiguous. Two extensions fix this
//! and both are accepted:
//!
//! - **X-FEN**: `K`/`Q` mean the outermost rook on that side of the king; a file letter
//!   (`A`–`H`, lowercase for Black) names the rook only when it isn't the outermost one.
//! - **Shredder-FEN**: always the rook's file letter, like `HAha`.
//!
//! cb2026 writes X-FEN, which is plain `KQkq` in standard chess.
//!
//! ## What gets checked
//!
//! Parsing rejects malformed text and positions that can't be played from: a missing or extra
//! king, pawns on the first or last rank, castling rights without the king and rook in place,
//! and an en passant square with no pawn that could have just made a double step. (Stage 2,
//! with attack generation, adds "the side that just moved must not be in check".)
//!
//! See the Chess Programming Wiki, [Forsyth-Edwards Notation](https://www.chessprogramming.org/Forsyth-Edwards_Notation)
//! and [X-FEN](https://www.chessprogramming.org/X-FEN).

use std::fmt;
use std::str::FromStr;

use crate::castling::{CastlingRights, CastlingSide};
use crate::piece::{Color, Piece, PieceType};
use crate::position::Position;
use crate::square::{File, Rank, Square};

/// The standard starting position.
pub const STARTING_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Why a FEN string could not be turned into a position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FenError {
    /// The string didn't have 4 or 6 space-separated fields.
    FieldCount(usize),
    /// The piece placement didn't have exactly 8 ranks.
    RankCount(usize),
    /// A rank in the piece placement didn't describe exactly 8 squares.
    RankLength {
        /// The rank, 1 to 8.
        rank: char,
        /// The rank's text.
        text: String,
    },
    /// A character in the piece placement is neither a piece letter nor a digit 1–8.
    BadPieceChar(char),
    /// The side to move wasn't `w` or `b`.
    BadSideToMove(String),
    /// The castling field couldn't be read, or names a right without its king and rook.
    BadCastling(String),
    /// The en passant field isn't `-` or a plausible en passant square.
    BadEnPassant(String),
    /// A move counter isn't a number.
    BadCounter(String),
    /// A side doesn't have exactly one king.
    KingCount {
        /// The side.
        color: Color,
        /// How many kings it has.
        count: u32,
    },
    /// A pawn stands on the first or last rank, which can never happen in a game.
    PawnOnBackRank(Square),
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FenError::FieldCount(n) => write!(
                f,
                "expected 6 fields (or 4 without the move counters), found {n}"
            ),
            FenError::RankCount(n) => write!(f, "expected 8 ranks separated by '/', found {n}"),
            FenError::RankLength { rank, text } => {
                write!(
                    f,
                    "rank {rank} ('{text}') doesn't describe exactly 8 squares"
                )
            }
            FenError::BadPieceChar(c) => write!(
                f,
                "'{c}' is not a piece letter (PNBRQK, pnbrqk) or a digit 1-8"
            ),
            FenError::BadSideToMove(s) => write!(f, "side to move must be 'w' or 'b', not '{s}'"),
            FenError::BadCastling(s) => write!(f, "bad castling rights: {s}"),
            FenError::BadEnPassant(s) => write!(f, "bad en passant square: {s}"),
            FenError::BadCounter(s) => write!(f, "'{s}' is not a valid move counter"),
            FenError::KingCount { color, count } => {
                write!(f, "{color} must have exactly one king, not {count}")
            }
            FenError::PawnOnBackRank(square) => {
                write!(
                    f,
                    "pawn on {square}: pawns can't stand on the first or last rank"
                )
            }
        }
    }
}

impl std::error::Error for FenError {}

impl Position {
    /// Parses a position from FEN (standard, X-FEN or Shredder-FEN castling).
    ///
    /// ```
    /// use cb2026_chess::{Position, Square, Color, PieceType, Piece};
    ///
    /// let pos = Position::from_fen("4k3/8/8/8/4P3/8/8/4K3 b - e3 0 1").unwrap();
    /// assert_eq!(pos.piece_at(Square::E4), Some(Piece::new(Color::White, PieceType::Pawn)));
    /// assert_eq!(pos.en_passant(), Some(Square::E3));
    /// ```
    pub fn from_fen(fen: &str) -> Result<Position, FenError> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if fields.len() != 4 && fields.len() != 6 {
            return Err(FenError::FieldCount(fields.len()));
        }

        let mut pos = Position::empty();
        parse_placement(&mut pos, fields[0])?;
        check_pieces(&pos)?;

        let side = match fields[1] {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(FenError::BadSideToMove(other.to_string())),
        };
        pos.set_side_to_move(side);

        pos.set_castling(parse_castling(&pos, fields[2])?);
        pos.set_en_passant(parse_en_passant(&pos, fields[3])?);

        if fields.len() == 6 {
            let halfmove = parse_counter(fields[4])?;
            // Some tools write a fullmove number of 0; treat it as 1.
            let fullmove = parse_counter(fields[5])?.max(1);
            pos.set_clocks(halfmove, fullmove);
        }

        debug_assert!(pos.is_consistent());
        Ok(pos)
    }

    /// Writes the position as FEN (X-FEN castling, which is plain `KQkq` in standard chess).
    pub fn to_fen(&self) -> String {
        let mut fen = String::new();

        for rank in Rank::ALL.iter().rev() {
            let mut empty = 0;
            for file in File::ALL {
                match self.piece_at(Square::new(file, *rank)) {
                    Some(piece) => {
                        if empty > 0 {
                            fen.push_str(&empty.to_string());
                            empty = 0;
                        }
                        fen.push(piece.to_fen_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                fen.push_str(&empty.to_string());
            }
            if *rank != Rank::First {
                fen.push('/');
            }
        }

        fen.push(' ');
        fen.push(self.side_to_move().to_char());
        fen.push(' ');
        fen.push_str(&castling_to_fen(self));
        fen.push(' ');
        match self.en_passant() {
            Some(square) => fen.push_str(&square.to_string()),
            None => fen.push('-'),
        }
        fen.push_str(&format!(
            " {} {}",
            self.halfmove_clock(),
            self.fullmove_number()
        ));
        fen
    }
}

impl FromStr for Position {
    type Err = FenError;

    fn from_str(s: &str) -> Result<Position, FenError> {
        Position::from_fen(s)
    }
}

/// Reads the piece placement field into `pos`.
fn parse_placement(pos: &mut Position, placement: &str) -> Result<(), FenError> {
    let ranks: Vec<&str> = placement.split('/').collect();
    if ranks.len() != 8 {
        return Err(FenError::RankCount(ranks.len()));
    }
    // FEN lists rank 8 first.
    for (text, rank) in ranks.iter().zip(Rank::ALL.iter().rev()) {
        let mut file = 0;
        for c in text.chars() {
            let too_long = || FenError::RankLength {
                rank: rank.to_char(),
                text: text.to_string(),
            };
            if let Some(skip) = c.to_digit(10).filter(|d| (1..=8).contains(d)) {
                file += skip as usize;
                if file > 8 {
                    return Err(too_long());
                }
            } else {
                let piece = Piece::from_fen_char(c).ok_or(FenError::BadPieceChar(c))?;
                let square_file = File::from_index(file).ok_or_else(too_long)?;
                pos.put_piece(Square::new(square_file, *rank), piece);
                file += 1;
            }
        }
        if file != 8 {
            return Err(FenError::RankLength {
                rank: rank.to_char(),
                text: text.to_string(),
            });
        }
    }
    Ok(())
}

/// Checks the piece placement on its own: one king each, no pawns on the back ranks.
fn check_pieces(pos: &Position) -> Result<(), FenError> {
    for color in Color::ALL {
        let count = pos.pieces(color, PieceType::King).count();
        if count != 1 {
            return Err(FenError::KingCount { color, count });
        }
    }
    let back_ranks = crate::Bitboard::rank(Rank::First) | crate::Bitboard::rank(Rank::Eighth);
    if let Some(square) = (pos.pieces_of_type(PieceType::Pawn) & back_ranks).first() {
        return Err(FenError::PawnOnBackRank(square));
    }
    Ok(())
}

/// The rank a colour's pieces start on.
fn back_rank(color: Color) -> Rank {
    match color {
        Color::White => Rank::First,
        Color::Black => Rank::Eighth,
    }
}

/// The files of `color`'s rooks on its back rank, a to h.
fn back_rank_rook_files(pos: &Position, color: Color) -> Vec<File> {
    (pos.pieces(color, PieceType::Rook) & crate::Bitboard::rank(back_rank(color)))
        .squares()
        .map(Square::file)
        .collect()
}

/// The outermost rook on `side` of the king, which `K`/`Q` refer to in X-FEN.
fn outermost_rook(pos: &Position, color: Color, side: CastlingSide, king: File) -> Option<File> {
    let rooks = back_rank_rook_files(pos, color);
    match side {
        CastlingSide::KingSide => rooks.into_iter().filter(|&f| f > king).max(),
        CastlingSide::QueenSide => rooks.into_iter().filter(|&f| f < king).min(),
    }
}

fn parse_castling(pos: &Position, field: &str) -> Result<CastlingRights, FenError> {
    let mut rights = CastlingRights::NONE;
    if field == "-" {
        return Ok(rights);
    }
    for c in field.chars() {
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };
        let king = pos.king_square(color);
        if king.rank() != back_rank(color) {
            return Err(FenError::BadCastling(format!(
                "'{c}' needs the {color} king on its back rank, but it is on {king}"
            )));
        }
        let (side, rook_file) = match c.to_ascii_lowercase() {
            'k' | 'q' => {
                let side = if c.eq_ignore_ascii_case(&'k') {
                    CastlingSide::KingSide
                } else {
                    CastlingSide::QueenSide
                };
                let file = outermost_rook(pos, color, side, king.file()).ok_or_else(|| {
                    FenError::BadCastling(format!(
                        "'{c}' but {color} has no rook on that side of its king"
                    ))
                })?;
                (side, file)
            }
            letter @ 'a'..='h' => {
                let file = File::from_char(letter).expect("a-h is a file");
                if !back_rank_rook_files(pos, color).contains(&file) {
                    return Err(FenError::BadCastling(format!(
                        "'{c}' but {color} has no rook on {}",
                        Square::new(file, back_rank(color))
                    )));
                }
                let side = if file > king.file() {
                    CastlingSide::KingSide
                } else {
                    CastlingSide::QueenSide
                };
                (side, file)
            }
            _ => {
                return Err(FenError::BadCastling(format!(
                    "'{c}' is not K, Q, k, q or a file letter"
                )));
            }
        };
        if rights.has(color, side) {
            return Err(FenError::BadCastling(format!(
                "'{field}' gives {color} two rights on the same side"
            )));
        }
        rights.set(color, side, rook_file);
    }
    Ok(rights)
}

fn castling_to_fen(pos: &Position) -> String {
    let rights = pos.castling();
    let mut out = String::new();
    for color in Color::ALL {
        for side in CastlingSide::ALL {
            let Some(file) = rights.rook_file(color, side) else {
                continue;
            };
            let king = pos.king_square(color).file();
            let c = if outermost_rook(pos, color, side, king) == Some(file) {
                match side {
                    CastlingSide::KingSide => 'k',
                    CastlingSide::QueenSide => 'q',
                }
            } else {
                file.to_char()
            };
            out.push(match color {
                Color::White => c.to_ascii_uppercase(),
                Color::Black => c,
            });
        }
    }
    if out.is_empty() {
        out.push('-');
    }
    out
}

fn parse_en_passant(pos: &Position, field: &str) -> Result<Option<Square>, FenError> {
    if field == "-" {
        return Ok(None);
    }
    let square: Square = field
        .parse()
        .map_err(|e: crate::square::ParseSquareError| FenError::BadEnPassant(e.to_string()))?;

    // The pawn that just moved belongs to the side *not* to move. It skipped `square`, so it
    // now stands one rank further on, and the square it started from is empty.
    let mover = !pos.side_to_move();
    let (expected_rank, pawn_rank, start_rank) = match mover {
        Color::White => (Rank::Third, Rank::Fourth, Rank::Second),
        Color::Black => (Rank::Sixth, Rank::Fifth, Rank::Seventh),
    };
    if square.rank() != expected_rank {
        return Err(FenError::BadEnPassant(format!(
            "{square} can't be an en passant square with {} to move",
            pos.side_to_move()
        )));
    }
    let pawn_square = Square::new(square.file(), pawn_rank);
    let start_square = Square::new(square.file(), start_rank);
    let pawn_there = pos.piece_at(pawn_square) == Some(Piece::new(mover, PieceType::Pawn));
    if !pawn_there || pos.piece_at(square).is_some() || pos.piece_at(start_square).is_some() {
        return Err(FenError::BadEnPassant(format!(
            "{square} needs a {mover} pawn on {pawn_square} that just moved from {start_square}"
        )));
    }
    Ok(Some(square))
}

fn parse_counter(field: &str) -> Result<u32, FenError> {
    field
        .parse()
        .map_err(|_| FenError::BadCounter(field.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Positions that must survive a FEN round trip unchanged.
    const ROUND_TRIP: &[&str] = &[
        STARTING_FEN,
        // "Kiwipete", a standard perft test position full of tricky moves.
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        // Chess960: rooks on b and g are the outermost, so plain KQkq.
        "brkqnnrb/pppppppp/8/8/8/8/PPPPPPPP/BRKQNNRB w KQkq - 0 1",
        // Castling with an inner rook (b1, with another rook on a1): X-FEN names it by file.
        "2k5/8/8/8/8/8/8/RRK5 w B - 0 1",
    ];

    #[test]
    fn round_trips() {
        for fen in ROUND_TRIP {
            let pos = Position::from_fen(fen).unwrap_or_else(|e| panic!("{fen}: {e}"));
            assert!(pos.is_consistent(), "{fen}");
            assert_eq!(pos.to_fen(), *fen);
        }
    }

    #[test]
    fn four_fields_default_the_counters() {
        let pos = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - -").unwrap();
        assert_eq!(pos.halfmove_clock(), 0);
        assert_eq!(pos.fullmove_number(), 1);
    }

    #[test]
    fn shredder_fen_castling() {
        let pos =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w HAha - 0 1").unwrap();
        assert_eq!(pos.castling(), CastlingRights::STANDARD);
        // Written back as X-FEN.
        assert_eq!(pos.to_fen(), STARTING_FEN);
    }

    #[test]
    fn chess960_castling_files() {
        let pos = Position::from_fen("rkr5/8/8/8/8/8/8/RKR5 w KQkq - 0 1").unwrap();
        let rights = pos.castling();
        assert_eq!(
            rights.rook_file(Color::White, CastlingSide::KingSide),
            Some(File::C)
        );
        assert_eq!(
            rights.rook_file(Color::White, CastlingSide::QueenSide),
            Some(File::A)
        );
        assert_eq!(
            rights.rook_file(Color::Black, CastlingSide::KingSide),
            Some(File::C)
        );
    }

    #[test]
    fn rejects_malformed_fen() {
        let cases = [
            ("", FenError::FieldCount(0)),
            ("8/8/8/8/8/8/8 w - - 0 1", FenError::RankCount(7)),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNRR w KQkq - 0 1",
                FenError::RankLength {
                    rank: '1',
                    text: "RNBQKBNRR".to_string(),
                },
            ),
            (
                "rnbqkbnr/ppppxppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                FenError::BadPieceChar('x'),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq - 0 1",
                FenError::BadSideToMove("x".to_string()),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - zero 1",
                FenError::BadCounter("zero".to_string()),
            ),
        ];
        for (fen, expected) in cases {
            assert_eq!(Position::from_fen(fen), Err(expected), "{fen:?}");
        }
    }

    #[test]
    fn rejects_impossible_positions() {
        assert_eq!(
            Position::from_fen("8/8/8/8/8/8/8/4K3 w - - 0 1"),
            Err(FenError::KingCount {
                color: Color::Black,
                count: 0
            })
        );
        assert_eq!(
            Position::from_fen("4k3/8/8/8/8/8/8/3PK3 w - - 0 1"),
            Err(FenError::PawnOnBackRank(Square::D1))
        );
        // Castling without the rook.
        assert!(matches!(
            Position::from_fen("4k3/8/8/8/8/8/8/4K3 w K - 0 1"),
            Err(FenError::BadCastling(_))
        ));
        // Castling with the king off its back rank.
        assert!(matches!(
            Position::from_fen("4k3/8/8/8/8/8/4K3/7R w K - 0 1"),
            Err(FenError::BadCastling(_))
        ));
        // En passant square on the wrong rank, and with no pawn that could have just moved.
        assert!(matches!(
            Position::from_fen("4k3/8/8/8/4P3/8/8/4K3 w - e3 0 1"),
            Err(FenError::BadEnPassant(_))
        ));
        assert!(matches!(
            Position::from_fen("4k3/8/8/8/8/8/8/4K3 b - e3 0 1"),
            Err(FenError::BadEnPassant(_))
        ));
    }

    #[test]
    fn errors_explain_themselves() {
        let err = Position::from_fen("4k3/8/8/8/8/8/8/3PK3 w - - 0 1").unwrap_err();
        assert_eq!(
            err.to_string(),
            "pawn on d1: pawns can't stand on the first or last rank"
        );
    }
}
