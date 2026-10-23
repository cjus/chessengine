//! Training records: one position from a self-play game, packed into 32 bytes.
//!
//! The trainer learns from hundreds of millions of positions, each with the search's score and
//! the game's result. Generating them takes the test Mac about 12 hours per 100 million, so the
//! format has to keep everything a later network might want, and stay small: 100 million
//! records are 3.2 GB.
//!
//! ## What a record keeps
//!
//! - The **whole position**: every piece, the side to move, castling rights (Chess960 too), the
//!   en passant square and the halfmove clock. Only the fullmove number is left out; a record
//!   turned back into a position says move 1. The network's inputs are computed from the
//!   position when training, so a network with different inputs can train on the same records.
//! - The **search score** in centipawns and the **game result**, both from White's side.
//! - The **best move**, so records can be filtered again later (say, to drop positions whose
//!   best move is a capture).
//!
//! ## The 32 bytes
//!
//! All little-endian:
//!
//! | Bytes | Field |
//! | --- | --- |
//! | 0–7 | the occupied squares, a bitboard (bit 0 is a1) |
//! | 8–23 | one 4-bit code per occupied square, in square order from a1; the first square's code is the lowest 4 bits |
//! | 24 | the side to move (bit 7: 0 White, 1 Black) and the en passant square (bits 0–6: 0–63, or 64 for none) |
//! | 25 | the halfmove clock (at most 255) |
//! | 26–27 | the search score, centipawns, White's side (`i16`) |
//! | 28 | the result: 0 Black won, 1 draw, 2 White won |
//! | 29–30 | the best move, in cb2026's 16-bit encoding ([`Move::to_bits`]) |
//! | 31 | the format version: 1 |
//!
//! A piece's code is its type (pawn 0 to king 5) plus 8 for Black. Code 6 (14 for Black) is a
//! rook that can still castle, which is how castling rights are kept: in Chess960 they belong to
//! particular rooks, and this names them. A position has at most 32 pieces, so 16 bytes of codes
//! are enough.
//!
//! Records have no file header, so files can be joined, split and shuffled freely. The version
//! byte in every record lets a reader tell a format it doesn't know.
//!
//! The layout follows marlinformat, the training format of Marlin's trainer marlinflow, whose
//! repository states no licence; only the idea is used, not its code. cb2026 keeps the best move
//! where marlinformat keeps the fullmove number, and a version byte in its spare byte.

use cb2026_chess::{Bitboard, CastlingSide, Color, Move, Piece, PieceType, Position, Rank, Square};

/// The size of a record in bytes.
pub const RECORD_LEN: usize = 32;
/// The format version this crate reads and writes.
const VERSION: u8 = 1;
/// The most pieces a record can hold: 32 four-bit codes.
const MAX_PIECES: u32 = 32;
/// A piece code's colour bit: set for Black.
const BLACK: u8 = 8;
/// The piece code (before the colour bit) of a rook that can still castle.
const CASTLING_ROOK: u8 = 6;
/// The en passant field's value when there is no en passant square.
const NO_EN_PASSANT: u8 = 64;

/// A game's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameResult {
    /// Black won.
    BlackWin,
    /// A draw.
    Draw,
    /// White won.
    WhiteWin,
}

impl GameResult {
    /// The result as a score for White: 0, ½ or 1.
    pub fn for_white(self) -> f32 {
        match self {
            GameResult::BlackWin => 0.0,
            GameResult::Draw => 0.5,
            GameResult::WhiteWin => 1.0,
        }
    }

    const fn code(self) -> u8 {
        match self {
            GameResult::BlackWin => 0,
            GameResult::Draw => 1,
            GameResult::WhiteWin => 2,
        }
    }
}

/// Why bytes aren't a record, or a position can't become one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordError {
    /// The record's version byte isn't one this cb2026 knows.
    UnsupportedVersion(u8),
    /// More than 32 pieces: they don't fit in a record.
    TooManyPieces(u32),
    /// A piece code that means nothing (7, 15, or a castling rook off its back rank).
    BadPiece(u8),
    /// The en passant field isn't a square or "none".
    BadEnPassant(u8),
    /// The result byte isn't 0, 1 or 2.
    BadResult(u8),
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::UnsupportedVersion(v) => {
                write!(
                    f,
                    "record format version {v}; this cb2026 reads version {VERSION}"
                )
            }
            RecordError::TooManyPieces(n) => write!(f, "{n} pieces; a record holds at most 32"),
            RecordError::BadPiece(code) => write!(f, "piece code {code} means nothing here"),
            RecordError::BadEnPassant(ep) => write!(f, "en passant field {ep} isn't a square"),
            RecordError::BadResult(r) => write!(f, "result {r} isn't 0, 1 or 2"),
        }
    }
}

impl std::error::Error for RecordError {}

/// One training position, packed (see the module docs for the layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    occupied: u64,
    /// 4-bit piece codes, one per occupied square in square order, the first in the low bits.
    codes: u128,
    /// The side to move (bit 7) and the en passant square (bits 0–6).
    turn: u8,
    halfmove_clock: u8,
    score: i16,
    result: GameResult,
    best_move: u16,
}

impl Record {
    // ANCHOR: from_position
    /// The record of `pos`, where the search chose `best_move` with a score of `score`
    /// centipawns from White's side, in a game that ended in `result`. A score beyond an `i16`
    /// is clamped.
    pub fn new(
        pos: &Position,
        score: i32,
        result: GameResult,
        best_move: Move,
    ) -> Result<Record, RecordError> {
        let occupied = pos.occupied();
        if occupied.count() > MAX_PIECES {
            return Err(RecordError::TooManyPieces(occupied.count()));
        }
        let castling_rooks = castling_rooks(pos);
        let mut codes = 0u128;
        for (i, square) in occupied.squares().enumerate() {
            let piece = pos
                .piece_at(square)
                .expect("an occupied square has a piece");
            let kind = if castling_rooks.contains(square) {
                CASTLING_ROOK
            } else {
                piece.piece_type.index() as u8
            };
            let code = kind
                | if piece.color == Color::Black {
                    BLACK
                } else {
                    0
                };
            codes |= u128::from(code) << (4 * i);
        }
        let side = if pos.side_to_move() == Color::Black {
            0x80
        } else {
            0
        };
        let en_passant = pos.en_passant().map_or(NO_EN_PASSANT, |s| s.index() as u8);
        Ok(Record {
            occupied: occupied.0,
            codes,
            turn: side | en_passant,
            halfmove_clock: pos.halfmove_clock().min(255) as u8,
            score: score.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
            result,
            best_move: best_move.to_bits(),
        })
    }
    // ANCHOR_END: from_position

    // ANCHOR: pieces
    /// Every piece and its square, in square order from a1. This is all the trainer needs to
    /// compute the network's inputs.
    pub fn pieces(&self) -> impl Iterator<Item = (Piece, Square)> + '_ {
        Bitboard(self.occupied)
            .squares()
            .enumerate()
            .map(|(i, square)| (piece_of(self.code(i)), square))
    }
    // ANCHOR_END: pieces

    /// The `i`-th occupied square's piece code.
    fn code(&self, i: usize) -> u8 {
        ((self.codes >> (4 * i)) & 0xf) as u8
    }

    /// The side to move.
    pub fn side_to_move(&self) -> Color {
        if self.turn & 0x80 != 0 {
            Color::Black
        } else {
            Color::White
        }
    }

    /// The search's score in centipawns, from White's side.
    pub fn score(&self) -> i16 {
        self.score
    }

    /// The search's score in centipawns, from the side to move's side, as the network sees it.
    pub fn score_for_side_to_move(&self) -> i16 {
        match self.side_to_move() {
            Color::White => self.score,
            Color::Black => self.score.saturating_neg(),
        }
    }

    /// The game's result.
    pub fn result(&self) -> GameResult {
        self.result
    }

    /// The move the search chose.
    pub fn best_move(&self) -> Move {
        Move::from_bits(self.best_move)
    }

    /// The position again. Its fullmove number is 1: records don't keep it.
    pub fn to_position(&self) -> Position {
        let mut board = [None; 64];
        let mut castling = String::new();
        for (i, (piece, square)) in self.pieces().enumerate() {
            board[square.index()] = Some(piece);
            if self.code(i) & !BLACK == CASTLING_ROOK {
                let letter = square.file().to_char();
                castling.push(match piece.color {
                    Color::White => letter.to_ascii_uppercase(),
                    Color::Black => letter,
                });
            }
        }
        // FEN lists White's rights first; a file letter names the castling rook, as in
        // Shredder-FEN, which works for Chess960 and standard chess alike.
        castling = sorted_rights(&castling);
        if castling.is_empty() {
            castling.push('-');
        }
        let mut ranks = Vec::with_capacity(8);
        for rank in Rank::ALL.iter().rev() {
            let mut text = String::new();
            let mut empty = 0;
            for square in Square::all().filter(|s| s.rank() == *rank) {
                match board[square.index()] {
                    Some(piece) => {
                        if empty > 0 {
                            text.push_str(&empty.to_string());
                            empty = 0;
                        }
                        text.push(piece.to_fen_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                text.push_str(&empty.to_string());
            }
            ranks.push(text);
        }
        let side = match self.side_to_move() {
            Color::White => 'w',
            Color::Black => 'b',
        };
        let en_passant = match self.turn & 0x7f {
            NO_EN_PASSANT => "-".to_string(),
            index => Square::from_index(index.into())
                .expect("checked")
                .to_string(),
        };
        let fen = format!(
            "{} {side} {castling} {en_passant} {} 1",
            ranks.join("/"),
            self.halfmove_clock
        );
        Position::from_fen(&fen).unwrap_or_else(|e| panic!("record gives a bad FEN {fen}: {e}"))
    }

    // ANCHOR: bytes
    /// The record's 32 bytes (see the module docs).
    pub fn to_bytes(&self) -> [u8; RECORD_LEN] {
        let mut bytes = [0u8; RECORD_LEN];
        bytes[0..8].copy_from_slice(&self.occupied.to_le_bytes());
        bytes[8..24].copy_from_slice(&self.codes.to_le_bytes());
        bytes[24] = self.turn;
        bytes[25] = self.halfmove_clock;
        bytes[26..28].copy_from_slice(&self.score.to_le_bytes());
        bytes[28] = self.result.code();
        bytes[29..31].copy_from_slice(&self.best_move.to_le_bytes());
        bytes[31] = VERSION;
        bytes
    }

    /// Reads a record, checking every field so the trainer never sees a meaningless position.
    pub fn from_bytes(bytes: &[u8; RECORD_LEN]) -> Result<Record, RecordError> {
        if bytes[31] != VERSION {
            return Err(RecordError::UnsupportedVersion(bytes[31]));
        }
        let occupied = u64::from_le_bytes(bytes[0..8].try_into().expect("8 bytes"));
        let count = occupied.count_ones();
        if count > MAX_PIECES {
            return Err(RecordError::TooManyPieces(count));
        }
        let codes = u128::from_le_bytes(bytes[8..24].try_into().expect("16 bytes"));
        for (i, square) in Bitboard(occupied).squares().enumerate() {
            let code = ((codes >> (4 * i)) & 0xf) as u8;
            let kind = code & !BLACK;
            let back_rank = if code & BLACK == 0 {
                Rank::First
            } else {
                Rank::Eighth
            };
            if kind > CASTLING_ROOK || (kind == CASTLING_ROOK && square.rank() != back_rank) {
                return Err(RecordError::BadPiece(code));
            }
        }
        let turn = bytes[24];
        if turn & 0x7f > NO_EN_PASSANT {
            return Err(RecordError::BadEnPassant(turn & 0x7f));
        }
        let result = match bytes[28] {
            0 => GameResult::BlackWin,
            1 => GameResult::Draw,
            2 => GameResult::WhiteWin,
            other => return Err(RecordError::BadResult(other)),
        };
        Ok(Record {
            occupied,
            codes,
            turn,
            halfmove_clock: bytes[25],
            score: i16::from_le_bytes([bytes[26], bytes[27]]),
            result,
            best_move: u16::from_le_bytes([bytes[29], bytes[30]]),
        })
    }
    // ANCHOR_END: bytes
}

/// The squares of the rooks that can still castle.
fn castling_rooks(pos: &Position) -> Bitboard {
    let rights = pos.castling();
    let mut rooks = Bitboard(0);
    for color in Color::ALL {
        let rank = match color {
            Color::White => Rank::First,
            Color::Black => Rank::Eighth,
        };
        for side in CastlingSide::ALL {
            if let Some(file) = rights.rook_file(color, side) {
                rooks = rooks.with(Square::new(file, rank));
            }
        }
    }
    rooks
}

/// The piece a (checked) code stands for: a castling rook is a rook.
fn piece_of(code: u8) -> Piece {
    let color = if code & BLACK == 0 {
        Color::White
    } else {
        Color::Black
    };
    let kind = code & !BLACK;
    let piece_type = if kind == CASTLING_ROOK {
        PieceType::Rook
    } else {
        PieceType::ALL[usize::from(kind)]
    };
    Piece::new(color, piece_type)
}

/// Castling letters with White's (uppercase) first, as FEN expects.
fn sorted_rights(letters: &str) -> String {
    let mut letters: Vec<char> = letters.chars().collect();
    letters.sort_by_key(|c| c.is_ascii_lowercase());
    letters.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use cb2026_chess::STARTING_FEN;

    use super::*;
    use crate::features;
    use crate::test_support::{random_game_positions, special_move_positions};

    /// A position's FEN with the fullmove number left out, which records don't keep.
    fn fen_without_fullmove(pos: &Position) -> String {
        let fen = pos.to_fen();
        fen.rsplit_once(' ')
            .map(|(rest, _)| rest.to_string())
            .unwrap()
    }

    fn record(pos: &Position, score: i32, result: GameResult) -> Record {
        let best = pos.legal_moves()[0];
        Record::new(pos, score, result, best).unwrap()
    }

    #[test]
    fn a_record_is_32_bytes_and_keeps_the_whole_position() {
        let positions = special_move_positions()
            .into_iter()
            .chain(random_game_positions(31, 40, 160))
            .filter(|p| !p.legal_moves().is_empty());
        for pos in positions {
            let rec = record(&pos, -123, GameResult::Draw);
            let bytes = rec.to_bytes();
            assert_eq!(bytes.len(), 32);
            let back = Record::from_bytes(&bytes).unwrap();
            assert_eq!(back, rec);
            // Castling rights (Chess960 too), en passant, the clock and the side to move survive.
            assert_eq!(
                fen_without_fullmove(&back.to_position()),
                fen_without_fullmove(&pos)
            );
            assert_eq!(back.best_move(), pos.legal_moves()[0]);
        }
    }

    #[test]
    fn the_inputs_come_straight_from_the_record() {
        // The trainer never builds a position: the record's pieces give the same inputs.
        for pos in random_game_positions(32, 10, 120)
            .iter()
            .filter(|p| !p.legal_moves().is_empty())
        {
            let rec = record(pos, 0, GameResult::WhiteWin);
            for view in Color::ALL {
                let mut from_record: Vec<usize> = rec
                    .pieces()
                    .map(|(p, s)| features::index(view, p, s))
                    .collect();
                let mut from_position: Vec<usize> = features::active(pos, view).collect();
                from_record.sort_unstable();
                from_position.sort_unstable();
                assert_eq!(from_record, from_position);
            }
        }
    }

    #[test]
    fn scores_and_results_are_kept_from_whites_side() {
        let mut pos = Position::from_fen(STARTING_FEN).unwrap();
        pos.make_move(pos.legal_moves()[0]);
        let rec = record(&pos, 40, GameResult::WhiteWin);
        assert_eq!(rec.side_to_move(), Color::Black);
        assert_eq!(rec.score(), 40);
        assert_eq!(rec.score_for_side_to_move(), -40);
        assert_eq!(rec.result().for_white(), 1.0);
        // Scores beyond an i16 are clamped.
        assert_eq!(record(&pos, 100_000, GameResult::Draw).score(), i16::MAX);
    }

    #[test]
    fn the_start_positions_castling_rooks_are_marked() {
        let pos = Position::from_fen(STARTING_FEN).unwrap();
        let bytes = record(&pos, 0, GameResult::Draw).to_bytes();
        // a1, the first occupied square, holds a castling rook: code 6 in the lowest bits.
        assert_eq!(bytes[8] & 0xf, CASTLING_ROOK);
        // b1's knight is next.
        assert_eq!(bytes[8] >> 4, PieceType::Knight.index() as u8);
    }

    #[test]
    fn damaged_records_are_refused() {
        let pos = Position::from_fen(STARTING_FEN).unwrap();
        let good = record(&pos, 0, GameResult::Draw).to_bytes();
        let with = |at: usize, value: u8| {
            let mut bytes = good;
            bytes[at] = value;
            Record::from_bytes(&bytes)
        };
        assert_eq!(with(31, 2), Err(RecordError::UnsupportedVersion(2)));
        assert_eq!(with(28, 3), Err(RecordError::BadResult(3)));
        assert_eq!(with(24, 65), Err(RecordError::BadEnPassant(65)));
        // a1's code made 7, which is no piece.
        assert_eq!(with(8, (good[8] & 0xf0) | 7), Err(RecordError::BadPiece(7)));
        // A castling rook off its back rank: a2's pawn, the 9th occupied square, made one.
        let a2_pawn = 8;
        let byte = 8 + a2_pawn / 2;
        let mut bytes = good;
        bytes[byte] = (bytes[byte] & 0xf0) | CASTLING_ROOK;
        assert_eq!(
            Record::from_bytes(&bytes),
            Err(RecordError::BadPiece(CASTLING_ROOK))
        );
        // 33 pieces don't fit.
        let mut crowded = good;
        crowded[0..8].copy_from_slice(&0x0000_00ff_ffff_ffffu64.to_le_bytes());
        assert_eq!(
            Record::from_bytes(&crowded),
            Err(RecordError::TooManyPieces(40))
        );
    }
}
