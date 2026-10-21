//! A chess position: where the pieces are, plus the state that the pieces alone don't show.
//!
//! ## Two views of the board
//!
//! cb2026 stores the pieces twice, and keeps both copies in step:
//!
//! - **Bitboards**: one per piece type and one per colour. "Where are the white knights?" is
//!   `pieces(White, Knight)`: one AND of two bitboards. Move generation and evaluation mostly
//!   ask questions like this.
//! - **A mailbox**: a 64-entry array saying what stands on each square. "What is on e4?" is a
//!   single array read, which bitboards answer only by testing up to twelve sets.
//!
//! Keeping both costs a little extra work when a piece moves, and saves much more whenever the
//! engine asks either kind of question. Most bitboard engines do the same.
//!
//! ## The rest of the state
//!
//! A position is more than piece placement. These also decide which moves are legal or how
//! the game can end:
//!
//! - **Side to move.**
//! - **Castling rights** (see [`crate::castling`]): whether each king and rook have moved.
//! - **En passant square**: the square a pawn skipped over with a double step on the last move,
//!   where an enemy pawn could capture it.
//! - **Halfmove clock**: moves since the last capture or pawn move, for the fifty-move rule.
//! - **Fullmove number**: starts at 1 and goes up after each Black move.
//!
//! Together these are exactly what [FEN](crate::fen) records.
//!
//! See the Chess Programming Wiki, [Board Representation](https://www.chessprogramming.org/Board_Representation)
//! and [Mailbox](https://www.chessprogramming.org/Mailbox).

use std::fmt;

use crate::bitboard::Bitboard;
use crate::castling::CastlingRights;
use crate::piece::{Color, Piece, PieceType};
use crate::square::{File, Rank, Square};
use crate::zobrist;

// ANCHOR: position
/// A chess position.
///
/// Build one from FEN with [`Position::from_fen`], or start a game with
/// [`Position::startpos`].
#[derive(Clone, PartialEq, Eq)]
pub struct Position {
    /// One bitboard per piece type, both colours together (indexed by [`PieceType::index`]).
    by_type: [Bitboard; 6],
    /// One bitboard per colour, all piece types together (indexed by [`Color::index`]).
    by_color: [Bitboard; 2],
    /// What stands on each square (indexed by [`Square::index`]).
    mailbox: [Option<Piece>; 64],
    side_to_move: Color,
    castling: CastlingRights,
    en_passant: Option<Square>,
    halfmove_clock: u32,
    fullmove_number: u32,
    /// The Zobrist key, kept up to date by every change (see [`crate::zobrist`]).
    hash: u64,
}
// ANCHOR_END: position

impl Position {
    /// The standard starting position.
    pub fn startpos() -> Position {
        Position::from_fen(crate::fen::STARTING_FEN).expect("the starting FEN is valid")
    }

    // ANCHOR: chess960
    /// Chess960 starting position number `n` (0–959, Scharnagl's numbering; 518 is the standard
    /// starting position), or `None` if `n` is out of range.
    ///
    /// The back rank is built from `n` in mixed-radix steps: the light-squared bishop (4
    /// choices), the dark-squared bishop (4), the queen on one of the 6 empty squares, the two
    /// knights on 2 of the 5 left (10 ways), and the rooks and king, in that order, on the last
    /// three, so the king always stands between its rooks.
    pub fn chess960(n: u32) -> Option<Position> {
        if n >= 960 {
            return None;
        }
        let mut rank = [None; 8];
        let mut n = n as usize;
        rank[2 * (n % 4) + 1] = Some('B');
        n /= 4;
        rank[2 * (n % 4)] = Some('B');
        n /= 4;
        let empty = |rank: &[Option<char>; 8]| -> Vec<usize> {
            (0..8).filter(|&i| rank[i].is_none()).collect()
        };
        rank[empty(&rank)[n % 6]] = Some('Q');
        n /= 6;
        const KNIGHTS: [(usize, usize); 10] = [
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4),
            (1, 2),
            (1, 3),
            (1, 4),
            (2, 3),
            (2, 4),
            (3, 4),
        ];
        let (first, second) = KNIGHTS[n];
        let free = empty(&rank);
        rank[free[first]] = Some('N');
        rank[free[second]] = Some('N');
        for (square, piece) in empty(&rank).into_iter().zip(['R', 'K', 'R']) {
            rank[square] = Some(piece);
        }
        let white: String = rank
            .iter()
            .map(|p| p.expect("every square filled"))
            .collect();
        // Shredder-FEN castling: the files of the rooks.
        let rook_files: Vec<char> = (0..8)
            .filter(|&i| rank[i] == Some('R'))
            .map(|i| (b'A' + i as u8) as char)
            .collect();
        let castling: String = rook_files
            .iter()
            .rev()
            .copied()
            .chain(rook_files.iter().rev().map(|c| c.to_ascii_lowercase()))
            .collect();
        let fen = format!(
            "{}/pppppppp/8/8/8/8/PPPPPPPP/{white} w {castling} - 0 1",
            white.to_ascii_lowercase()
        );
        Position::from_fen(&fen).ok()
    }
    // ANCHOR_END: chess960

    /// A board with no pieces, White to move, no castling rights. Not a legal position on its
    /// own; FEN parsing fills it in.
    pub(crate) fn empty() -> Position {
        Position {
            by_type: [Bitboard::EMPTY; 6],
            by_color: [Bitboard::EMPTY; 2],
            mailbox: [None; 64],
            side_to_move: Color::White,
            castling: CastlingRights::NONE,
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            hash: 0,
        }
    }

    // ANCHOR: put_piece
    /// Puts `piece` on the empty square `square`, updating both views of the board.
    pub(crate) fn put_piece(&mut self, square: Square, piece: Piece) {
        debug_assert!(
            self.mailbox[square.index()].is_none(),
            "{square} is occupied"
        );
        self.mailbox[square.index()] = Some(piece);
        self.by_type[piece.piece_type.index()] |= Bitboard::from_square(square);
        self.by_color[piece.color.index()] |= Bitboard::from_square(square);
        self.hash ^= zobrist::piece_square(piece, square);
    }
    // ANCHOR_END: put_piece

    /// Removes the piece on `square` and returns it, updating both views of the board.
    pub(crate) fn remove_piece(&mut self, square: Square) -> Option<Piece> {
        let piece = self.mailbox[square.index()].take()?;
        self.by_type[piece.piece_type.index()] &= !Bitboard::from_square(square);
        self.by_color[piece.color.index()] &= !Bitboard::from_square(square);
        self.hash ^= zobrist::piece_square(piece, square);
        Some(piece)
    }

    /// The position's Zobrist key: equal positions (same pieces, side to move, castling
    /// rights and en passant square) have equal keys.
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// The Zobrist key computed from scratch. [`Position::hash`] must always equal it; tests
    /// use this to check the incremental updates.
    pub fn compute_hash(&self) -> u64 {
        let mut hash = 0;
        for square in Square::all() {
            if let Some(piece) = self.piece_at(square) {
                hash ^= zobrist::piece_square(piece, square);
            }
        }
        if self.side_to_move == Color::Black {
            hash ^= zobrist::black_to_move();
        }
        hash ^= zobrist::castling(self.castling);
        if let Some(square) = self.en_passant {
            hash ^= zobrist::en_passant(square.file());
        }
        hash
    }

    /// What stands on `square`, if anything.
    pub fn piece_at(&self, square: Square) -> Option<Piece> {
        self.mailbox[square.index()]
    }

    /// The squares holding `color`'s pieces of type `piece_type`.
    pub fn pieces(&self, color: Color, piece_type: PieceType) -> Bitboard {
        self.by_color[color.index()] & self.by_type[piece_type.index()]
    }

    /// The squares holding pieces of type `piece_type`, of either colour.
    pub fn pieces_of_type(&self, piece_type: PieceType) -> Bitboard {
        self.by_type[piece_type.index()]
    }

    /// The squares holding `color`'s pieces.
    pub fn pieces_of_color(&self, color: Color) -> Bitboard {
        self.by_color[color.index()]
    }

    /// All occupied squares.
    pub fn occupied(&self) -> Bitboard {
        self.by_color[0] | self.by_color[1]
    }

    /// The square of `color`'s king.
    ///
    /// Every position built through this crate has exactly one king per side, so this always
    /// finds one.
    pub fn king_square(&self, color: Color) -> Square {
        self.pieces(color, PieceType::King)
            .first()
            .expect("every position has a king of each colour")
    }

    /// Whose turn it is.
    pub fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    /// The remaining castling rights.
    pub fn castling(&self) -> CastlingRights {
        self.castling
    }

    /// The en passant target square, if the last move was a pawn's double step.
    pub fn en_passant(&self) -> Option<Square> {
        self.en_passant
    }

    /// Halfmoves since the last capture or pawn move (the fifty-move rule counter).
    pub fn halfmove_clock(&self) -> u32 {
        self.halfmove_clock
    }

    /// The move number, starting at 1 and increasing after each Black move.
    pub fn fullmove_number(&self) -> u32 {
        self.fullmove_number
    }

    pub(crate) fn set_side_to_move(&mut self, color: Color) {
        if color != self.side_to_move {
            self.hash ^= zobrist::black_to_move();
        }
        self.side_to_move = color;
    }

    pub(crate) fn set_castling(&mut self, castling: CastlingRights) {
        self.hash ^= zobrist::castling(self.castling) ^ zobrist::castling(castling);
        self.castling = castling;
    }

    pub(crate) fn set_en_passant(&mut self, square: Option<Square>) {
        if let Some(old) = self.en_passant {
            self.hash ^= zobrist::en_passant(old.file());
        }
        if let Some(new) = square {
            self.hash ^= zobrist::en_passant(new.file());
        }
        self.en_passant = square;
    }

    pub(crate) fn set_clocks(&mut self, halfmove_clock: u32, fullmove_number: u32) {
        self.halfmove_clock = halfmove_clock;
        self.fullmove_number = fullmove_number;
    }

    /// Checks that the bitboards, the mailbox and the hash key all describe the same position.
    ///
    /// Every function that changes the board keeps them in step; this is the test that they
    /// do. It is cheap enough for tests and debug assertions, not for the search.
    pub fn is_consistent(&self) -> bool {
        let mut by_type = [Bitboard::EMPTY; 6];
        let mut by_color = [Bitboard::EMPTY; 2];
        for square in Square::all() {
            if let Some(piece) = self.mailbox[square.index()] {
                by_type[piece.piece_type.index()] |= Bitboard::from_square(square);
                by_color[piece.color.index()] |= Bitboard::from_square(square);
            }
        }
        let types_disjoint = PieceType::ALL.iter().enumerate().all(|(i, a)| {
            PieceType::ALL[i + 1..]
                .iter()
                .all(|b| (self.by_type[a.index()] & self.by_type[b.index()]).is_empty())
        });
        by_type == self.by_type
            && by_color == self.by_color
            && types_disjoint
            && (self.by_color[0] & self.by_color[1]).is_empty()
            && self.hash == self.compute_hash()
    }
}

/// The dark squares: a1, c1, …, b2, d2, … (a1 is dark).
const DARK_SQUARES: Bitboard = Bitboard(0xaa55_aa55_aa55_aa55);

impl Position {
    // ANCHOR: insufficient_material
    /// Whether neither side has enough material to checkmate, so the game is a draw: king
    /// against king, king and one minor piece against king, or kings and bishops where every
    /// bishop stands on the same colour of square.
    pub fn is_insufficient_material(&self) -> bool {
        let heavy = self.pieces_of_type(PieceType::Pawn)
            | self.pieces_of_type(PieceType::Rook)
            | self.pieces_of_type(PieceType::Queen);
        if !heavy.is_empty() {
            return false;
        }
        let knights = self.pieces_of_type(PieceType::Knight);
        let bishops = self.pieces_of_type(PieceType::Bishop);
        let minors = (knights | bishops).count();
        if minors <= 1 {
            return true;
        }
        // Only bishops, all on light squares or all on dark squares.
        knights.is_empty()
            && ((bishops & DARK_SQUARES).is_empty() || (bishops & !DARK_SQUARES).is_empty())
    }
    // ANCHOR_END: insufficient_material

    // ANCHOR: mating_material
    /// Whether `color` has the material to checkmate by some series of legal moves, however
    /// the other side plays. A player whose flag falls loses only if the opponent has
    /// (FIDE Laws 6.9); otherwise the game is a draw.
    ///
    /// It answers `false` only when the material makes a mate impossible:
    ///
    /// - **A lone king.**
    /// - **A king and one knight, against a king and nothing but queens.** A knight's check
    ///   can't be blocked, so for mate no queen may be able to take the knight. Put the mated
    ///   king at (0, 0) and the knight at (1, 2) (every other knight's check is a reflection
    ///   of this one). The king's neighbours (0, 1) and (1, 1) touch the knight, which
    ///   attacks neither, and a queen on either would take it, so the attacking king must
    ///   cover both, and the only square beside both that is neither beside the mated king
    ///   nor the knight's own is (0, 2). Nothing then covers (1, 0), so a queen must stand
    ///   there, and it takes the knight through (1, 1).
    /// - **A king and bishops all on one colour, against a king with no pawns, no knights and
    ///   no bishops of the other colour.** The mated king stands on the bishops' colour,
    ///   checked along a diagonal through its neighbour D (where the bishop may stand). The
    ///   two neighbours beside both the king and D are of the other colour, so no bishop
    ///   covers them, and the attacking king can't cover both: the only squares beside both
    ///   are the mated king's and D. So one holds a piece of the defender's, and the only
    ///   ones allowed there, a rook or a queen, step onto D, blocking the check or taking the
    ///   bishop. Two bishops of one colour never give double check, which nothing could
    ///   answer that way: a bishop leaving another's line to the king moves along a diagonal
    ///   parallel to the king's other one, so it can't check as well.
    ///
    /// Neither side can ever get material of a kind it lacks without a pawn, so the answer
    /// holds for the rest of the game. It is `true` in every other case, even when the
    /// position is dead for another reason, such as locked pawns: those are rare, and seeing
    /// them takes a search. When neither side has the material, the position is
    /// [insufficient material](Position::is_insufficient_material).
    pub fn has_mating_material(&self, color: Color) -> bool {
        let own = self.pieces_of_color(color);
        let theirs = self.pieces_of_color(!color);
        let pawns = self.pieces_of_type(PieceType::Pawn);
        let knights = self.pieces_of_type(PieceType::Knight);
        let bishops = self.pieces_of_type(PieceType::Bishop);
        let heavy =
            pawns | self.pieces_of_type(PieceType::Rook) | self.pieces_of_type(PieceType::Queen);
        if !(own & heavy).is_empty() {
            return true;
        }
        let own_knights = own & knights;
        let own_bishops = own & bishops;
        if own_bishops.is_empty() {
            // A lone king, or knights.
            let theirs_besides_queens = theirs
                & !self.pieces_of_type(PieceType::King)
                & !self.pieces_of_type(PieceType::Queen);
            return !own_knights.is_empty()
                && (own_knights.count() > 1 || !theirs_besides_queens.is_empty());
        }
        if !own_knights.is_empty() {
            return true;
        }
        let dark = own_bishops & DARK_SQUARES;
        if !dark.is_empty() && dark != own_bishops {
            return true;
        }
        let other_colour = if dark.is_empty() {
            DARK_SQUARES
        } else {
            !DARK_SQUARES
        };
        !(theirs & (pawns | knights | (bishops & other_colour))).is_empty()
    }
    // ANCHOR_END: mating_material
}

impl Default for Position {
    /// The standard starting position.
    fn default() -> Position {
        Position::startpos()
    }
}

impl fmt::Display for Position {
    /// A plain-text diagram (rank 8 at the top, `.` for empty squares) followed by the FEN.
    ///
    /// The shell draws richer boards; this one is for tests, logs and debugging.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in Rank::ALL.iter().rev() {
            write!(f, "{} ", rank.to_char())?;
            for file in File::ALL {
                let c = self
                    .piece_at(Square::new(file, *rank))
                    .map_or('.', Piece::to_fen_char);
                write!(f, " {c}")?;
            }
            writeln!(f)?;
        }
        writeln!(f, "   a b c d e f g h")?;
        write!(f, "{}", self.to_fen())
    }
}

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Position({})", self.to_fen())
    }
}

#[cfg(test)]
mod chess960_tests {
    use super::*;

    #[test]
    fn number_518_is_the_standard_position() {
        let pos = Position::chess960(518).unwrap();
        assert_eq!(pos.occupied(), Position::startpos().occupied());
        assert_eq!(pos.hash(), Position::startpos().hash());
    }

    #[test]
    fn known_numbers_and_the_rules() {
        // Scharnagl's table: 0 is BBQNNRKR.
        let zero = Position::chess960(0).unwrap();
        assert!(
            zero.to_fen()
                .starts_with("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w ")
        );
        assert!(Position::chess960(960).is_none());
        let mut seen = std::collections::HashSet::new();
        for n in 0..960 {
            let pos = Position::chess960(n).unwrap();
            assert!(pos.is_consistent(), "{n}");
            // Distinct, with bishops on opposite colours and the king between the rooks.
            assert!(seen.insert(pos.to_fen()), "{n}");
            let back = pos.to_fen().split('/').next_back().unwrap()[..8].to_string();
            let bishops: Vec<usize> = back.match_indices('B').map(|(i, _)| i).collect();
            assert_ne!(bishops[0] % 2, bishops[1] % 2, "{n}: {back}");
            let king = back.find('K').unwrap();
            assert!(
                back[..king].contains('R') && back[king..].contains('R'),
                "{n}: {back}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_pieces() {
        let pos = Position::startpos();
        assert!(pos.is_consistent());
        assert_eq!(pos.occupied().count(), 32);
        assert_eq!(
            pos.pieces(Color::White, PieceType::Pawn),
            Bitboard::rank(Rank::Second)
        );
        assert_eq!(
            pos.pieces(Color::Black, PieceType::Pawn),
            Bitboard::rank(Rank::Seventh)
        );
        assert_eq!(pos.king_square(Color::White), Square::E1);
        assert_eq!(pos.king_square(Color::Black), Square::E8);
        assert_eq!(
            pos.piece_at(Square::D8),
            Some(Piece::new(Color::Black, PieceType::Queen))
        );
        assert_eq!(pos.piece_at(Square::E4), None);
        assert_eq!(pos.side_to_move(), Color::White);
        assert_eq!(pos.castling(), CastlingRights::STANDARD);
        assert_eq!(pos.en_passant(), None);
        assert_eq!(pos.halfmove_clock(), 0);
        assert_eq!(pos.fullmove_number(), 1);
    }

    #[test]
    fn piece_counts_by_type() {
        let pos = Position::startpos();
        let expected = [16, 4, 4, 4, 2, 2];
        for (piece_type, count) in PieceType::ALL.into_iter().zip(expected) {
            assert_eq!(
                pos.pieces_of_type(piece_type).count(),
                count,
                "{piece_type:?}"
            );
        }
        assert_eq!(pos.pieces_of_color(Color::White), Bitboard(0xffff));
    }

    #[test]
    fn insufficient_material() {
        let draw = |fen: &str| Position::from_fen(fen).unwrap().is_insufficient_material();
        assert!(draw("4k3/8/8/8/8/8/8/4K3 w - - 0 1"), "two kings");
        assert!(draw("4k3/8/8/8/8/8/8/4KN2 w - - 0 1"), "one knight");
        assert!(draw("4k3/8/8/8/8/8/8/4KB2 w - - 0 1"), "one bishop");
        assert!(
            draw("2b1k3/8/8/8/8/8/8/4KB2 w - - 0 1"),
            "bishops on the same colour"
        );
        assert!(
            !draw("1b2k3/8/8/8/8/8/8/4KB2 w - - 0 1"),
            "opposite-coloured bishops"
        );
        assert!(
            !draw("4k3/8/8/8/8/8/8/3NKN2 w - - 0 1"),
            "two knights can't force mate but can mate"
        );
        assert!(!draw("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"), "a pawn");
        assert!(!Position::startpos().is_insufficient_material());
    }

    #[test]
    fn mating_material() {
        let can =
            |fen: &str, color: Color| Position::from_fen(fen).unwrap().has_mating_material(color);
        let (white, black) = (Color::White, Color::Black);
        assert!(!can("4k3/8/8/8/8/8/8/4K3 w - - 0 1", white), "a lone king");
        assert!(!can("3qk3/8/8/8/8/8/8/4K3 w - - 0 1", white), "a lone king");
        assert!(can("3qk3/8/8/8/8/8/8/4K3 w - - 0 1", black), "a queen");
        assert!(!can("4k3/8/8/8/8/8/8/4KN2 w - - 0 1", white), "a knight");
        assert!(
            !can("3qk3/8/8/8/8/8/8/4KN2 w - - 0 1", white),
            "a knight and a queen"
        );
        assert!(
            can("3rk3/8/8/8/8/8/8/4KN2 w - - 0 1", white),
            "a knight and a rook"
        );
        assert!(
            can("4k3/4p3/8/8/8/8/8/4KN2 w - - 0 1", white),
            "a knight and a pawn"
        );
        assert!(can("4k3/8/8/8/8/8/8/3NKN2 w - - 0 1", white), "two knights");
        assert!(!can("4k3/8/8/8/8/8/8/4KB2 w - - 0 1", white), "a bishop");
        assert!(
            !can("3rk3/8/8/8/8/8/8/4KB2 w - - 0 1", white),
            "a bishop and a rook"
        );
        assert!(
            !can("3qk3/8/8/8/8/8/8/3BKB2 w - - 0 1", white),
            "light bishops, a queen"
        );
        assert!(
            !can("2b1k3/8/8/8/8/8/8/4KB2 w - - 0 1", white),
            "light bishops"
        );
        assert!(
            can("1b2k3/8/8/8/8/8/8/4KB2 w - - 0 1", white),
            "opposite bishops"
        );
        assert!(
            can("3nk3/8/8/8/8/8/8/4KB2 w - - 0 1", white),
            "a bishop and a knight"
        );
        assert!(
            can("4k3/4p3/8/8/8/8/8/4KB2 w - - 0 1", white),
            "a bishop and a pawn"
        );
        assert!(
            can("4k3/8/8/8/8/8/8/2B1KB2 w - - 0 1", white),
            "bishops on both colours"
        );
        assert!(
            can("4k3/8/8/8/8/8/8/2N1KB2 w - - 0 1", white),
            "a bishop and a knight"
        );
        assert!(can("4k3/8/8/8/8/8/8/R3K3 w - - 0 1", white), "a rook");
        assert!(can("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", white), "a pawn");
    }

    #[test]
    fn the_mates_that_make_material_enough() {
        // A mate for each kind of "can" above with a single minor piece.
        let mate = |fen: &str| {
            let pos = Position::from_fen(fen).unwrap();
            pos.in_check() && pos.legal_moves().is_empty()
        };
        assert!(
            mate("6rk/5N2/7K/8/8/8/8/8 b - - 0 1"),
            "a knight against a rook"
        );
        assert!(
            mate("k1K5/p1N5/8/8/8/8/8/8 b - - 0 1"),
            "a knight against a pawn"
        );
        assert!(
            mate("7k/5KBn/8/8/8/8/8/8 b - - 0 1"),
            "a bishop against a knight"
        );
        assert!(
            mate("8/8/8/3B4/8/8/5K1p/7k b - - 0 1"),
            "a bishop against a pawn"
        );
    }

    #[test]
    fn insufficient_material_is_neither_side_having_mating_material() {
        for fen in [
            "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/4KN2 w - - 0 1",
            "4k3/8/8/8/8/8/8/4KB2 w - - 0 1",
            "2b1k3/8/8/8/8/8/8/4KB2 w - - 0 1",
            "1b2k3/8/8/8/8/8/8/4KB2 w - - 0 1",
            "3nk3/8/8/8/8/8/8/4KN2 w - - 0 1",
            "3qk3/8/8/8/8/8/8/4KN2 w - - 0 1",
            "3rk3/8/8/8/8/8/8/4KB2 w - - 0 1",
            "4k3/8/8/8/8/8/8/3NKN2 w - - 0 1",
        ] {
            let pos = Position::from_fen(fen).unwrap();
            let neither =
                !pos.has_mating_material(Color::White) && !pos.has_mating_material(Color::Black);
            assert_eq!(pos.is_insufficient_material(), neither, "{fen}");
        }
    }

    #[test]
    fn displays_a_diagram() {
        let text = Position::startpos().to_string();
        assert!(text.starts_with("8  r n b q k b n r\n"));
        assert!(text.contains("1  R N B Q K B N R\n   a b c d e f g h\n"));
        assert!(text.ends_with(crate::fen::STARTING_FEN));
    }
}
