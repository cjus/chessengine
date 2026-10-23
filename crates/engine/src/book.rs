//! Opening books: moves chosen by people (or other programs) for the first moves of a game.
//!
//! A search in the opening spends its time rediscovering centuries of opening theory, and
//! plays the same move every time in the same position. An **opening book** maps positions to
//! good moves, with a weight for how often each should be played. The engine looks the
//! position up before searching and, if the book has moves, plays one at once, chosen at random
//! in proportion to the weights. That saves time on the clock and gives variety.
//!
//! cb2026 reads the **Polyglot** format (`.bin` files), the most widely used: a sorted list of
//! 16-byte entries (position key, move, weight, learning data), all big-endian. The position
//! key is a Zobrist hash, but with the format's own fixed random numbers (see
//! [`polyglot_key`]), so any program can read any book. Lookup is a binary search on the key.
//! It also writes books ([`OpeningBook::from_entries`], [`OpeningBook::to_bytes`]), for the
//! `make-book` tool, which builds one from people's games (`cb2026_tools::book_maker`).
//!
//! The book is off by default (UCI `OwnBook`), because testing needs the engine's own moves:
//! a book would make test games start the same way and hide what the search does. GUIs and
//! tournament managers usually supply their own book anyway.
//!
//! See the Chess Programming Wiki, [Opening Book](https://www.chessprogramming.org/Opening_Book)
//! and [PolyGlot](https://www.chessprogramming.org/PolyGlot).

use cb2026_chess::{CastlingSide, Color, Move, PieceType, Position, Square};

use crate::book_keys::POLYGLOT_KEYS;

/// The bytes of one book entry.
const ENTRY_SIZE: usize = 16;

// ANCHOR: key
/// The Polyglot key of `pos`: the XOR of the format's random numbers for every piece on its
/// square, each castling right, the en passant file (only when a pawn could actually capture
/// there) and White to move.
pub fn polyglot_key(pos: &Position) -> u64 {
    let mut key = 0;
    for square in pos.occupied() {
        let piece = pos.piece_at(square).expect("an occupied square");
        // The format's piece kinds: black pawn, white pawn, black knight, white knight, ...
        let kind = 2 * piece.piece_type.index() + usize::from(piece.color == Color::White);
        key ^= POLYGLOT_KEYS[64 * kind + square.index()];
    }
    let rights = [
        (Color::White, CastlingSide::KingSide),
        (Color::White, CastlingSide::QueenSide),
        (Color::Black, CastlingSide::KingSide),
        (Color::Black, CastlingSide::QueenSide),
    ];
    for (i, (color, side)) in rights.into_iter().enumerate() {
        if pos.castling().has(color, side) {
            key ^= POLYGLOT_KEYS[768 + i];
        }
    }
    // cb2026 only records an en passant square when an enemy pawn can capture there, which is
    // exactly the Polyglot rule.
    if let Some(square) = pos.en_passant() {
        key ^= POLYGLOT_KEYS[772 + square.file().index()];
    }
    if pos.side_to_move() == Color::White {
        key ^= POLYGLOT_KEYS[780];
    }
    key
}
// ANCHOR_END: key

/// A move from the book, with its weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookMove {
    /// The move.
    pub mv: Move,
    /// How often to play it, relative to the other moves for the position.
    pub weight: u16,
}

/// A Polyglot opening book, loaded in memory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpeningBook {
    /// `(key, move, weight)`, sorted by key as in the file.
    entries: Vec<(u64, u16, u16)>,
}

impl OpeningBook {
    /// A book from `(key, move, weight)` entries in any order, with moves encoded by
    /// [`encode_move`].
    pub fn from_entries(mut entries: Vec<(u64, u16, u16)>) -> OpeningBook {
        entries.sort_unstable();
        OpeningBook { entries }
    }

    /// The book as the bytes of a `.bin` file, sorted by key, with the learning data zero.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.entries.len() * ENTRY_SIZE);
        for &(key, mv, weight) in &self.entries {
            bytes.extend(key.to_be_bytes());
            bytes.extend(mv.to_be_bytes());
            bytes.extend(weight.to_be_bytes());
            bytes.extend([0; 4]);
        }
        bytes
    }

    /// Reads a book from the bytes of a `.bin` file.
    pub fn from_bytes(bytes: &[u8]) -> Result<OpeningBook, String> {
        if !bytes.len().is_multiple_of(ENTRY_SIZE) {
            return Err(format!(
                "not a Polyglot book: {} bytes is not a whole number of 16-byte entries",
                bytes.len()
            ));
        }
        let entries: Vec<(u64, u16, u16)> = bytes
            .chunks_exact(ENTRY_SIZE)
            .map(|e| {
                let key = u64::from_be_bytes(e[0..8].try_into().expect("8 bytes"));
                let mv = u16::from_be_bytes([e[8], e[9]]);
                let weight = u16::from_be_bytes([e[10], e[11]]);
                (key, mv, weight)
            })
            .collect();
        if entries.windows(2).any(|w| w[0].0 > w[1].0) {
            return Err("not a Polyglot book: entries are not sorted by key".to_string());
        }
        Ok(OpeningBook { entries })
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the book is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    // ANCHOR: moves
    /// The book's moves for `pos`, heaviest first (illegal ones, from a corrupt book or a key
    /// collision, are skipped).
    pub fn moves(&self, pos: &Position) -> Vec<BookMove> {
        let key = polyglot_key(pos);
        let start = self.entries.partition_point(|e| e.0 < key);
        let mut moves: Vec<BookMove> = self.entries[start..]
            .iter()
            .take_while(|e| e.0 == key)
            .filter_map(|&(_, raw, weight)| decode_move(pos, raw).map(|mv| BookMove { mv, weight }))
            .collect();
        moves.sort_by_key(|m| std::cmp::Reverse(m.weight));
        moves
    }
    // ANCHOR_END: moves

    /// Picks one of the book's moves for `pos` at random, in proportion to the weights, using
    /// `random` (any 64-bit number) as the dice. `None` if the book has nothing.
    pub fn pick(&self, pos: &Position, random: u64) -> Option<Move> {
        let moves = self.moves(pos);
        let total: u64 = moves.iter().map(|m| u64::from(m.weight)).sum();
        if total == 0 {
            return moves.first().map(|m| m.mv);
        }
        let mut roll = random % total;
        for m in &moves {
            if roll < u64::from(m.weight) {
                return Some(m.mv);
            }
            roll -= u64::from(m.weight);
        }
        None
    }
}

/// Encodes `mv` as a Polyglot move: to-square in bits 0–5, from-square in bits 6–11,
/// promotion piece in bits 12–14 (1 knight, 2 bishop, 3 rook, 4 queen). Castling is "king
/// takes rook" (`e1h1`), cb2026's own encoding, so it needs nothing special.
pub fn encode_move(mv: Move) -> u16 {
    let promotion = match mv.promotion() {
        None => 0,
        Some(PieceType::Knight) => 1,
        Some(PieceType::Bishop) => 2,
        Some(PieceType::Rook) => 3,
        Some(_) => 4,
    };
    promotion << 12 | (mv.from().index() as u16) << 6 | mv.to().index() as u16
}

/// Decodes a Polyglot move in `pos`, as [`encode_move`] encodes it.
fn decode_move(pos: &Position, raw: u16) -> Option<Move> {
    let square = |bits: u16| Square::from_index(usize::from(bits & 63));
    let to = square(raw)?;
    let from = square(raw >> 6)?;
    let promotion = match (raw >> 12) & 7 {
        0 => None,
        1 => Some(PieceType::Knight),
        2 => Some(PieceType::Bishop),
        3 => Some(PieceType::Rook),
        4 => Some(PieceType::Queen),
        _ => return None,
    };
    let mut uci = format!("{from}{to}");
    if let Some(piece) = promotion {
        uci.push(piece.to_char());
    }
    pos.parse_uci_move(&uci)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The format's published test positions and hashes.
    const TEST_HASHES: [(&str, u64); 9] = [
        ("", 0x463B96181691FC9C),
        ("e2e4", 0x823C9B50FD114196),
        ("e2e4 d7d5", 0x0756B94461C50FB0),
        ("e2e4 d7d5 e4e5", 0x662FAFB965DB29D4),
        ("e2e4 d7d5 e4e5 f7f5", 0x22A48B5A8E47FF78),
        ("e2e4 d7d5 e4e5 f7f5 e1e2", 0x652A607CA3F242C1),
        ("e2e4 d7d5 e4e5 f7f5 e1e2 e8f7", 0x00FDD303C946BDD9),
        ("a2a4 b7b5 h2h4 b5b4 c2c4", 0x3C8123EA7B067637),
        ("a2a4 b7b5 h2h4 b5b4 c2c4 b4c3 a1a3", 0x5C3F9B829B279560),
    ];

    fn play(moves: &str) -> Position {
        let mut pos = Position::startpos();
        for uci in moves.split_whitespace() {
            let mv = pos.parse_uci_move(uci).unwrap();
            pos.make_move(mv);
        }
        pos
    }

    /// A book's bytes from `(position moves, move, weight)` entries.
    fn book_bytes(entries: &[(&str, &str, u16)]) -> Vec<u8> {
        let rows = entries
            .iter()
            .map(|&(moves, uci, weight)| {
                let pos = play(moves);
                let mv = pos.parse_uci_move(uci).unwrap();
                (polyglot_key(&pos), encode_move(mv), weight)
            })
            .collect();
        OpeningBook::from_entries(rows).to_bytes()
    }

    #[test]
    fn keys_match_the_published_test_hashes() {
        for (moves, expected) in TEST_HASHES {
            assert_eq!(polyglot_key(&play(moves)), expected, "after '{moves}'");
        }
    }

    #[test]
    fn looks_up_and_picks_by_weight() {
        let book = OpeningBook::from_bytes(&book_bytes(&[
            ("", "e2e4", 30),
            ("", "d2d4", 10),
            ("e2e4", "c7c5", 1),
        ]))
        .unwrap();
        assert_eq!(book.len(), 3);
        let start = Position::startpos();
        let moves = book.moves(&start);
        assert_eq!(moves.len(), 2);
        assert_eq!(moves[0].mv.to_string(), "e2e4");
        // Rolls 0-29 pick e4, 30-39 pick d4.
        assert_eq!(book.pick(&start, 29).unwrap().to_string(), "e2e4");
        assert_eq!(book.pick(&start, 30).unwrap().to_string(), "d2d4");
        assert_eq!(book.pick(&start, 40 + 31).unwrap().to_string(), "d2d4");
        assert_eq!(book.pick(&play("e2e4"), 7).unwrap().to_string(), "c7c5");
        assert_eq!(book.pick(&play("d2d4"), 7), None);
    }

    #[test]
    fn castling_is_king_takes_rook() {
        let pos = Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        // e1 = 4, h1 = 7.
        let castle = decode_move(&pos, 4 << 6 | 7).unwrap();
        assert_eq!(pos.to_san(castle), "O-O");
    }

    #[test]
    fn moves_encode_and_decode() {
        // Castling (king takes rook), an under-promotion with a capture, and a quiet move.
        let pos = Position::from_fen("r3k3/1P6/8/8/8/8/8/R3K2R w KQq - 0 1").unwrap();
        for uci in ["e1h1", "e1a1", "b7a8n", "b7b8q", "a1a7"] {
            let mv = pos.parse_uci_move(uci).unwrap();
            assert_eq!(decode_move(&pos, encode_move(mv)), Some(mv), "{uci}");
        }
        // The format's numbers: e1 is 4, h1 is 7, and a knight promotion is 1.
        let castle = pos.parse_uci_move("e1h1").unwrap();
        assert_eq!(encode_move(castle), 4 << 6 | 7);
        let promotion = pos.parse_uci_move("b7a8n").unwrap();
        assert_eq!(encode_move(promotion), 1 << 12 | 49 << 6 | 56);
    }

    #[test]
    fn written_books_read_back() {
        let bytes = book_bytes(&[("e2e4", "c7c5", 3), ("", "e2e4", 2), ("", "d2d4", 1)]);
        assert_eq!(bytes.len(), 3 * ENTRY_SIZE);
        let book = OpeningBook::from_bytes(&bytes).unwrap();
        assert_eq!(book.to_bytes(), bytes);
        let first: Vec<String> = book
            .moves(&Position::startpos())
            .iter()
            .map(|m| format!("{} {}", m.mv, m.weight))
            .collect();
        assert_eq!(first, ["e2e4 2", "d2d4 1"]);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(OpeningBook::from_bytes(&[0; 15]).is_err());
        // Two entries with keys 2 then 1: out of order.
        let unsorted: Vec<u8> = [2u64, 1]
            .iter()
            .flat_map(|key| key.to_be_bytes().into_iter().chain([0; 8]))
            .collect();
        assert!(OpeningBook::from_bytes(&unsorted).is_err());
        assert!(OpeningBook::from_bytes(&[]).unwrap().is_empty());
    }
}
