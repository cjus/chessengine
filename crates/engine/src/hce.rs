//! The hand-crafted evaluation's positional terms (stage 15).
//!
//! Material and piece-square tables (see [`crate::eval`]) know what each piece is worth and
//! where it likes to stand, but nothing about how pieces relate to each other. This module
//! adds the classic terms that do:
//!
//! - **Mobility:** pieces with more safe squares to go to are worth more. A bishop hemmed in by
//!   its own pawns is a poor bishop. Each knight, bishop, rook and queen scores per square it
//!   attacks that isn't occupied by its own side and isn't covered by an enemy pawn, relative
//!   to a typical count.
//! - **Pawn structure:** doubled pawns (two on a file) and isolated pawns (no friendly pawn on
//!   a neighbouring file) are weak. **Passed pawns** (no enemy pawn ahead on the same or a
//!   neighbouring file) are strong, more so the further they've advanced, and most of all in
//!   the endgame.
//! - **Pieces:** the **bishop pair** is worth more than two single bishops. Rooks belong on
//!   **open files** (no pawns) or **semi-open** files (no pawns of their own).
//! - **King safety:** in the middlegame a king wants a **pawn shield** in front of it, and
//!   enemy pieces attacking the squares around it are dangerous. The danger grows faster than
//!   linearly with the number of attackers (one piece rarely mates alone), so it is scored as
//!   the square of weighted attack counts.
//! - **Tempo:** having the move is worth a little.
//!
//! Every term has a middlegame and an endgame value (see tapered evaluation in
//! [`crate::eval`]). All the numbers are cb2026's own starting values; stage 16 tunes them.
//!
//! See the Chess Programming Wiki, [Evaluation](https://www.chessprogramming.org/Evaluation),
//! [Mobility](https://www.chessprogramming.org/Mobility),
//! [Pawn Structure](https://www.chessprogramming.org/Pawn_Structure),
//! [Passed Pawn](https://www.chessprogramming.org/Passed_Pawn),
//! [Bishop Pair](https://www.chessprogramming.org/Bishop_Pair) and
//! [King Safety](https://www.chessprogramming.org/King_Safety).

use cb2026_chess::attacks::{
    bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks, rook_attacks,
};
use cb2026_chess::{Bitboard, Color, File, PieceType, Position, Rank, Square};

use crate::eval::{MgEg, Param, TermSink};

// ANCHOR: params
/// The positional terms' numbers (each `(middlegame, endgame)` in centipawns) and switches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermParams {
    /// Mobility: value per safe square, by piece type (pawn and king unused).
    pub mobility: [MgEg; 6],
    /// Mobility is counted relative to this typical number of safe squares, by piece type.
    pub mobility_typical: [i32; 6],
    /// Each pawn beyond the first on a file.
    pub doubled: MgEg,
    /// Each pawn with no friendly pawn on a neighbouring file.
    pub isolated: MgEg,
    /// A passed pawn, by rank from its own side (index 1 = second rank ... 6 = seventh).
    pub passed: [MgEg; 8],
    /// Two bishops or more.
    pub bishop_pair: MgEg,
    /// A rook on a file with no pawns.
    pub rook_open_file: MgEg,
    /// A rook on a file with no pawns of its own side.
    pub rook_semi_open_file: MgEg,
    /// Each friendly pawn in the 3×2 block in front of the king (at most 3 count).
    pub pawn_shield: MgEg,
    /// King danger units per attacked square around the king, by attacking piece type.
    pub king_attack_weights: [i32; 6],
    /// King danger penalty (middlegame) = units² × this / 100...
    pub king_danger_scale: i32,
    /// ...up to this much.
    pub king_danger_max: i32,
    /// For the side to move.
    pub tempo: MgEg,
    /// Score mobility.
    pub use_mobility: bool,
    /// Score pawn structure and passed pawns.
    pub use_pawns: bool,
    /// Score the bishop pair and rook files.
    pub use_pieces: bool,
    /// Score king safety.
    pub use_king_safety: bool,
    /// Score the tempo.
    pub use_tempo: bool,
}

impl Default for TermParams {
    /// The hand-set values (stage 15). The engine uses the tuned ones (see [`crate::tuned`]).
    fn default() -> TermParams {
        TermParams {
            mobility: [(0, 0), (4, 4), (5, 5), (2, 4), (1, 2), (0, 0)],
            mobility_typical: [0, 4, 6, 7, 13, 0],
            doubled: (-10, -20),
            isolated: (-10, -15),
            passed: [
                (0, 0),
                (0, 5),
                (5, 10),
                (10, 20),
                (20, 35),
                (35, 60),
                (60, 90),
                (0, 0),
            ],
            bishop_pair: (30, 50),
            rook_open_file: (25, 10),
            rook_semi_open_file: (12, 8),
            pawn_shield: (15, 0),
            king_attack_weights: [0, 2, 2, 3, 5, 0],
            king_danger_scale: 35,
            king_danger_max: 500,
            tempo: (10, 5),
            use_mobility: true,
            use_pawns: true,
            use_pieces: true,
            use_king_safety: true,
            use_tempo: true,
        }
    }
}

impl TermParams {
    /// All positional terms switched off.
    pub fn none() -> TermParams {
        TermParams {
            use_mobility: false,
            use_pawns: false,
            use_pieces: false,
            use_king_safety: false,
            use_tempo: false,
            ..TermParams::default()
        }
    }
}
// ANCHOR_END: params

/// The files next to `file` (not `file` itself).
fn adjacent_files(file: File) -> Bitboard {
    let i = file.index();
    let mut files = Bitboard::EMPTY;
    for neighbour in [i.wrapping_sub(1), i + 1] {
        if let Some(f) = File::from_index(neighbour) {
            files |= Bitboard::file(f);
        }
    }
    files
}

/// The ranks strictly ahead of `rank` from `color`'s side.
fn ranks_ahead(color: Color, rank: Rank) -> Bitboard {
    Rank::ALL
        .iter()
        .filter(|r| match color {
            Color::White => r.index() > rank.index(),
            Color::Black => r.index() < rank.index(),
        })
        .fold(Bitboard::EMPTY, |acc, &r| acc | Bitboard::rank(r))
}

/// `square`'s rank counted from `color`'s side (0 = its back rank).
fn relative_rank(color: Color, square: Square) -> usize {
    match color {
        Color::White => square.rank().index(),
        Color::Black => 7 - square.rank().index(),
    }
}

/// All squares attacked by `color`'s pawns.
fn pawn_attack_span(pos: &Position, color: Color) -> Bitboard {
    pos.pieces(color, PieceType::Pawn)
        .squares()
        .fold(Bitboard::EMPTY, |acc, sq| acc | pawn_attacks(color, sq))
}

/// The squares a piece of type `piece_type` on `square` attacks.
fn piece_attacks(piece_type: PieceType, square: Square, occupied: Bitboard) -> Bitboard {
    match piece_type {
        PieceType::Knight => knight_attacks(square),
        PieceType::Bishop => bishop_attacks(square, occupied),
        PieceType::Rook => rook_attacks(square, occupied),
        PieceType::Queen => queen_attacks(square, occupied),
        PieceType::King => king_attacks(square),
        PieceType::Pawn => Bitboard::EMPTY,
    }
}

const PIECES: [PieceType; 4] = [
    PieceType::Knight,
    PieceType::Bishop,
    PieceType::Rook,
    PieceType::Queen,
];

// ANCHOR: terms
/// Reports every switched-on positional term, for both sides, to `sink`: which parameter
/// applies and how many times (see [`TermSink`]).
pub fn collect<S: TermSink>(pos: &Position, p: &TermParams, sink: &mut S) {
    let occupied = pos.occupied();
    for us in Color::ALL {
        let them = !us;
        let our_pawns = pos.pieces(us, PieceType::Pawn);
        let their_pawns = pos.pieces(them, PieceType::Pawn);

        if p.use_mobility {
            // Safe squares: not our own pieces, not covered by an enemy pawn.
            let area = !pos.pieces_of_color(us) & !pawn_attack_span(pos, them);
            for pt in PIECES {
                for square in pos.pieces(us, pt) {
                    let count = (piece_attacks(pt, square, occupied) & area).count() as i32;
                    sink.term(
                        us,
                        Param::Mobility(pt),
                        count - p.mobility_typical[pt.index()],
                    );
                }
            }
        }

        if p.use_pawns {
            for square in our_pawns {
                let file = Bitboard::file(square.file());
                let neighbours = adjacent_files(square.file());
                // Doubled: another of our pawns further up the same file.
                if !(our_pawns & file & ranks_ahead(us, square.rank())).is_empty() {
                    sink.term(us, Param::Doubled, 1);
                }
                if (our_pawns & neighbours).is_empty() {
                    sink.term(us, Param::Isolated, 1);
                }
                // Passed: no enemy pawn ahead on this file or a neighbouring one.
                let ahead = (file | neighbours) & ranks_ahead(us, square.rank());
                if (their_pawns & ahead).is_empty() {
                    sink.term(us, Param::Passed(relative_rank(us, square)), 1);
                }
            }
        }

        if p.use_pieces {
            if pos.pieces(us, PieceType::Bishop).count() >= 2 {
                sink.term(us, Param::BishopPair, 1);
            }
            for square in pos.pieces(us, PieceType::Rook) {
                let file = Bitboard::file(square.file());
                if (file & (our_pawns | their_pawns)).is_empty() {
                    sink.term(us, Param::RookOpenFile, 1);
                } else if (file & our_pawns).is_empty() {
                    sink.term(us, Param::RookSemiOpenFile, 1);
                }
            }
        }

        if p.use_king_safety {
            let king = pos.king_square(us);
            // Pawn shield: our pawns on the king's file and its neighbours, one or two ranks
            // ahead of it.
            let in_front = ranks_ahead(us, king.rank()) & !ranks_ahead(us, two_ahead(us, king));
            let shield = our_pawns & (Bitboard::file(king.file()) | adjacent_files(king.file()));
            let shield_count = (shield & in_front).count().min(3) as i32;
            sink.term(us, Param::PawnShield, shield_count);

            // Attacks on the king zone, weighted by attacker; the penalty grows quadratically.
            let zone = king_attacks(king).with(king);
            let mut units = 0;
            for pt in PIECES {
                for square in pos.pieces(them, pt) {
                    let hits = (piece_attacks(pt, square, occupied) & zone).count() as i32;
                    units += hits * p.king_attack_weights[pt.index()];
                }
            }
            let danger = (units * units * p.king_danger_scale / 100).min(p.king_danger_max);
            sink.king_danger(us, danger);
        }
    }

    if p.use_tempo {
        sink.term(pos.side_to_move(), Param::Tempo, 1);
    }
}
// ANCHOR_END: terms

/// The square two ranks ahead of `king` (clamped to the board), for the shield's extent.
fn two_ahead(color: Color, king: Square) -> Rank {
    let index = match color {
        Color::White => (king.rank().index() + 2).min(7),
        Color::Black => king.rank().index().saturating_sub(2),
    };
    Rank::from_index(index).expect("a rank on the board")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(fen: &str) -> crate::eval::EvalTrace {
        crate::eval::trace(
            &Position::from_fen(fen).unwrap(),
            // The hand-set values, which the expectations below are written in terms of.
            &crate::eval::EvalParams::hand_set(),
        )
    }

    #[test]
    fn helpers() {
        assert_eq!(adjacent_files(File::A), Bitboard::file(File::B));
        assert_eq!(adjacent_files(File::D).count(), 16);
        assert_eq!(
            ranks_ahead(Color::White, Rank::Seventh),
            Bitboard::rank(Rank::Eighth)
        );
        assert_eq!(
            ranks_ahead(Color::Black, Rank::Second),
            Bitboard::rank(Rank::First)
        );
    }

    #[test]
    fn pawn_structure() {
        // White: doubled c-pawns, isolated h-pawn. Black: a passed pawn on b3.
        let t = terms("4k3/8/8/8/2P5/2P4P/1p6/4K3 w - - 0 1");
        let p = TermParams::default();
        let white = t.pawns[Color::White.index()];
        // c3 is behind c4: one doubled; c3, c4 and h3 are all isolated.
        assert_eq!(white.0, p.doubled.0 + 3 * p.isolated.0);
        // b2 is on Black's seventh rank, relative rank 6.
        assert_eq!(t.passed[Color::Black.index()], p.passed[6]);
    }

    #[test]
    fn bishop_pair_and_open_files() {
        let t = terms("4k3/pppp4/8/8/8/8/1P6/2BBK2R w - - 0 1");
        let p = TermParams::default();
        // h-file: no pawns at all.
        assert_eq!(
            t.pieces[Color::White.index()],
            (
                p.bishop_pair.0 + p.rook_open_file.0,
                p.bishop_pair.1 + p.rook_open_file.1
            )
        );
    }

    #[test]
    fn king_safety_prefers_a_shield_and_no_attackers() {
        let sheltered = terms("6k1/5ppp/8/8/8/8/5PPP/6K1 w - - 0 1");
        let bare = terms("6k1/8/8/8/8/8/5PPP/6K1 w - - 0 1");
        let black = Color::Black.index();
        assert!(sheltered.king_safety[black].0 > bare.king_safety[black].0);
        // A queen and rook bearing down on the white king.
        let attacked = terms("6k1/5ppp/8/8/8/6q1/5PP1/5rK1 w - - 0 1");
        let white = Color::White.index();
        assert!(attacked.king_safety[white].0 < sheltered.king_safety[white].0 - 50);
    }

    #[test]
    fn mobility_rewards_active_pieces() {
        let centre = terms("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1");
        let corner = terms("4k3/8/8/8/8/8/8/N3K3 w - - 0 1");
        let white = Color::White.index();
        assert!(centre.mobility[white].0 > corner.mobility[white].0);
    }
}
