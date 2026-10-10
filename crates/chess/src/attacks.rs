//! Attack tables: which squares each piece attacks.
//!
//! ## Leaping pieces: tables computed once
//!
//! A knight, king or pawn attacks the same squares from a given square no matter what else is
//! on the board. So each of their attack sets is computed once, at compile time, into a
//! 64-entry table, and "where does a knight on g1 attack?" becomes one array read:
//! `KNIGHT_ATTACKS[g1]`.
//!
//! ## Sliding pieces: walking rays
//!
//! Bishops, rooks and queens slide until they hit something, so their attacks depend on which
//! squares are occupied. Here they are computed the obvious way: from the piece, step in each
//! direction, adding squares, and stop after the first occupied square (which is attacked too,
//! whoever owns it). Stage 3 replaces this with *magic bitboards*, a table lookup that gives
//! the same answer much faster; this version stays as the reference the magics are tested
//! against.
//!
//! ## Lines between squares
//!
//! [`between()`] and [`line()`] answer geometric questions that legal move generation needs
//! constantly: which squares lie between a king and the piece checking it (a move to block
//! must land there), and which line a pinned piece must stay on.
//!
//! See the Chess Programming Wiki, [Knight Pattern](https://www.chessprogramming.org/Knight_Pattern),
//! [King Pattern](https://www.chessprogramming.org/King_Pattern),
//! [Pawn Attacks](https://www.chessprogramming.org/Pawn_Attacks_(Bitboards)) and
//! [Classical Approach](https://www.chessprogramming.org/Classical_Approach).

use std::sync::OnceLock;

use crate::bitboard::Bitboard;
use crate::piece::Color;
use crate::square::Square;

/// (file, rank) steps a knight can make.
const KNIGHT_STEPS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

/// (file, rank) steps a king can make; also the eight sliding directions.
const KING_STEPS: [(i8, i8); 8] = [
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];

/// Directions a rook slides in.
const ROOK_DIRECTIONS: [(i8, i8); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];

/// Directions a bishop slides in.
const BISHOP_DIRECTIONS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, -1), (-1, 1)];

/// The square one `(file, rank)` step from `index`, or `None` off the board.
const fn step(index: usize, (df, dr): (i8, i8)) -> Option<usize> {
    let file = (index % 8) as i8 + df;
    let rank = (index / 8) as i8 + dr;
    if file >= 0 && file < 8 && rank >= 0 && rank < 8 {
        Some((rank * 8 + file) as usize)
    } else {
        None
    }
}

/// The set of squares one step from `index` in each of `steps`.
const fn leaper_attacks(index: usize, steps: &[(i8, i8)]) -> u64 {
    let mut bits = 0;
    let mut i = 0;
    while i < steps.len() {
        if let Some(to) = step(index, steps[i]) {
            bits |= 1 << to;
        }
        i += 1;
    }
    bits
}

/// Builds a 64-entry table of leaper attacks.
const fn leaper_table(steps: &[(i8, i8)]) -> [Bitboard; 64] {
    let mut table = [Bitboard::EMPTY; 64];
    let mut index = 0;
    while index < 64 {
        table[index] = Bitboard(leaper_attacks(index, steps));
        index += 1;
    }
    table
}

// ANCHOR: leapers
/// `KNIGHT_ATTACKS[sq]`: the squares a knight on `sq` attacks.
pub const KNIGHT_ATTACKS: [Bitboard; 64] = leaper_table(&KNIGHT_STEPS);

/// `KING_ATTACKS[sq]`: the squares a king on `sq` attacks.
pub const KING_ATTACKS: [Bitboard; 64] = leaper_table(&KING_STEPS);
// ANCHOR_END: leapers

/// `PAWN_ATTACKS[color][sq]`: the squares a pawn of `color` on `sq` attacks (diagonally
/// forward; pawns don't attack the square they move to).
pub const PAWN_ATTACKS: [[Bitboard; 64]; 2] = [
    leaper_table(&[(-1, 1), (1, 1)]),
    leaper_table(&[(-1, -1), (1, -1)]),
];

/// The squares a knight on `square` attacks.
pub fn knight_attacks(square: Square) -> Bitboard {
    KNIGHT_ATTACKS[square.index()]
}

/// The squares a king on `square` attacks.
pub fn king_attacks(square: Square) -> Bitboard {
    KING_ATTACKS[square.index()]
}

/// The squares a pawn of `color` on `square` attacks.
pub fn pawn_attacks(color: Color, square: Square) -> Bitboard {
    PAWN_ATTACKS[color.index()][square.index()]
}

// ANCHOR: slider
/// Slides from `square` in each direction until the edge or the first occupied square
/// (included, since a piece there can be captured or is defended).
fn slider_attacks(square: Square, occupied: Bitboard, directions: &[(i8, i8)]) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &direction in directions {
        let mut index = square.index();
        while let Some(next) = step(index, direction) {
            let next_square = Square::from_index(next).expect("step stays on the board");
            attacks = attacks.with(next_square);
            if occupied.contains(next_square) {
                break;
            }
            index = next;
        }
    }
    attacks
}
// ANCHOR_END: slider

/// The squares a rook on `square` attacks, given the occupied squares.
pub fn rook_attacks(square: Square, occupied: Bitboard) -> Bitboard {
    slider_attacks(square, occupied, &ROOK_DIRECTIONS)
}

/// The squares a bishop on `square` attacks, given the occupied squares.
pub fn bishop_attacks(square: Square, occupied: Bitboard) -> Bitboard {
    slider_attacks(square, occupied, &BISHOP_DIRECTIONS)
}

/// The squares a queen on `square` attacks: a rook's and a bishop's together.
pub fn queen_attacks(square: Square, occupied: Bitboard) -> Bitboard {
    rook_attacks(square, occupied) | bishop_attacks(square, occupied)
}

/// `between[a][b]` and `line[a][b]` for every pair of squares.
struct LineTables {
    between: Vec<[Bitboard; 64]>,
    line: Vec<[Bitboard; 64]>,
}

/// Builds the line tables once, the first time they are needed.
fn line_tables() -> &'static LineTables {
    static TABLES: OnceLock<LineTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut between = vec![[Bitboard::EMPTY; 64]; 64];
        let mut line = vec![[Bitboard::EMPTY; 64]; 64];
        for from in 0..64 {
            for direction in KING_STEPS {
                // The whole line through `from` in this direction and its opposite.
                let opposite = (-direction.0, -direction.1);
                let ray = |dir| {
                    let mut squares = Bitboard::EMPTY;
                    let mut index = from;
                    while let Some(next) = step(index, dir) {
                        squares = squares.with(Square::from_index(next).expect("on board"));
                        index = next;
                    }
                    squares
                };
                let full_line = ray(direction)
                    | ray(opposite)
                    | Bitboard::from_square(Square::from_index(from).expect("on board"));

                let mut path = Bitboard::EMPTY;
                let mut index = from;
                while let Some(next) = step(index, direction) {
                    between[from][next] = path;
                    line[from][next] = full_line;
                    path = path.with(Square::from_index(next).expect("on board"));
                    index = next;
                }
            }
        }
        LineTables { between, line }
    })
}

/// The squares strictly between `a` and `b` if they share a rank, file or diagonal; otherwise
/// empty.
pub fn between(a: Square, b: Square) -> Bitboard {
    line_tables().between[a.index()][b.index()]
}

/// The whole rank, file or diagonal through `a` and `b` (edge to edge, including both), or
/// empty if they don't share one.
pub fn line(a: Square, b: Square) -> Bitboard {
    line_tables().line[a.index()][b.index()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(squares: &[Square]) -> Bitboard {
        squares.iter().copied().collect()
    }

    #[test]
    fn knight_attacks_in_the_corner_and_centre() {
        assert_eq!(knight_attacks(Square::A1), set(&[Square::B3, Square::C2]));
        assert_eq!(knight_attacks(Square::E4).count(), 8);
        assert_eq!(knight_attacks(Square::H8), set(&[Square::G6, Square::F7]));
    }

    #[test]
    fn king_attacks_count() {
        assert_eq!(king_attacks(Square::A1).count(), 3);
        assert_eq!(king_attacks(Square::E1).count(), 5);
        assert_eq!(king_attacks(Square::E4).count(), 8);
    }

    #[test]
    fn pawn_attacks_go_forward_diagonally() {
        assert_eq!(
            pawn_attacks(Color::White, Square::E4),
            set(&[Square::D5, Square::F5])
        );
        assert_eq!(
            pawn_attacks(Color::Black, Square::E4),
            set(&[Square::D3, Square::F3])
        );
        assert_eq!(pawn_attacks(Color::White, Square::A2), set(&[Square::B3]));
    }

    #[test]
    fn sliders_stop_at_blockers() {
        // Rook on d4 with blockers on d6 and f4: d6 and f4 are attacked, d7 and g4 are not.
        let occupied = set(&[Square::D6, Square::F4]);
        let attacks = rook_attacks(Square::D4, occupied);
        assert!(attacks.contains(Square::D6) && attacks.contains(Square::F4));
        assert!(!attacks.contains(Square::D7) && !attacks.contains(Square::G4));
        assert_eq!(attacks.count(), 2 + 2 + 3 + 3);
        assert_eq!(rook_attacks(Square::A1, Bitboard::EMPTY).count(), 14);
        assert_eq!(bishop_attacks(Square::D4, Bitboard::EMPTY).count(), 13);
        assert_eq!(queen_attacks(Square::D4, Bitboard::EMPTY).count(), 27);
    }

    #[test]
    fn between_and_line() {
        assert_eq!(
            between(Square::A1, Square::D4),
            set(&[Square::B2, Square::C3])
        );
        assert_eq!(
            between(Square::D4, Square::A1),
            between(Square::A1, Square::D4)
        );
        assert_eq!(between(Square::E1, Square::E2), Bitboard::EMPTY);
        assert_eq!(
            between(Square::A1, Square::B3),
            Bitboard::EMPTY,
            "not aligned"
        );
        assert_eq!(line(Square::C3, Square::E5).count(), 8, "the long diagonal");
        assert!(line(Square::C3, Square::E5).contains(Square::A1));
        assert_eq!(line(Square::A1, Square::B3), Bitboard::EMPTY);
        assert_eq!(line(Square::E2, Square::E7), Bitboard::file(crate::File::E));
    }
}
