//! Magic bitboards: sliding-piece attacks by table lookup.
//!
//! ## The problem
//!
//! A rook's attacks depend on which squares around it are occupied. Walking rays square by
//! square (see [`crate::attacks`]) is simple but slow: up to 14 steps with a branch each. The
//! attacks only depend on a handful of squares, though, so could we just look them up?
//!
//! ## The idea
//!
//! 1. **Mask.** Only the squares on the rook's rays matter, and not the last square of each
//!    ray: whether the edge square is occupied or not, the rook attacks it. For a rook on d4
//!    that leaves 10 *relevant* squares, so there are 2¹⁰ = 1024 possible occupancies to care
//!    about. `occupied & mask` keeps exactly those bits.
//! 2. **Hash.** We need to turn those scattered bits into a small table index. Multiply by a
//!    carefully chosen 64-bit **magic number** and keep the top bits:
//!    `index = ((occupied & mask) * magic) >> (64 - bits)`. The multiplication gathers the
//!    scattered bits into the top of the word. A magic number is "good" if no two occupancies
//!    that need *different* attack sets get the same index (two that need the same attack set
//!    may share one: a harmless "constructive" collision).
//! 3. **Look up.** Precompute the attack set for every occupancy into a table. Then
//!    `attacks = table[index]`: an AND, a multiply, a shift and a load.
//!
//! The magic numbers are found by trial ([`find_magic`]): generate random sparse 64-bit numbers
//! until one works for the square. That search takes a moment, so it is done once by the
//! `find-magics` tool, which writes the numbers into `magic/magic_numbers.rs`. At startup
//! cb2026 only fills in the tables for those known numbers, checking that every one still
//! works.
//!
//! All 128 tables (64 squares, rook and bishop) share one array, each square at its own
//! offset ("fancy" magic bitboards): 107,648 entries, about 840 KB.
//!
//! The search method follows Tord Romstad's public-domain example on the Chess Programming
//! Wiki ("Looking for Magics").
//!
//! See the Chess Programming Wiki, [Magic Bitboards](https://www.chessprogramming.org/Magic_Bitboards)
//! and [Looking for Magics](https://www.chessprogramming.org/Looking_for_Magics).

use std::sync::OnceLock;

use crate::attacks::{bishop_attacks_by_rays, rook_attacks_by_rays};
use crate::bitboard::Bitboard;
use crate::square::{File, Rank, Square};

#[rustfmt::skip]
mod magic_numbers;

/// A function computing a slider's attacks the slow, sure way (walking rays).
type Reference = fn(Square, Bitboard) -> Bitboard;

// ANCHOR: magic
/// The magic data for one square and one kind of slider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Magic {
    /// The relevant squares: the slider's rays, without the edge squares.
    pub mask: Bitboard,
    /// The magic multiplier.
    pub magic: u64,
    /// `64 - (number of relevant squares)`.
    pub shift: u32,
    /// Where this square's part of the shared attack table starts.
    pub offset: usize,
}

impl Magic {
    /// The attack-table index for the given occupied squares.
    pub fn index(&self, occupied: Bitboard) -> usize {
        let relevant = (occupied & self.mask).0;
        self.offset + (relevant.wrapping_mul(self.magic) >> self.shift) as usize
    }
}
// ANCHOR_END: magic

/// Every square's magics and the shared attack table.
struct MagicTables {
    rook: [Magic; 64],
    bishop: [Magic; 64],
    attacks: Vec<Bitboard>,
}

fn tables() -> &'static MagicTables {
    static TABLES: OnceLock<MagicTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        build_tables(&magic_numbers::ROOK_MAGICS, &magic_numbers::BISHOP_MAGICS)
            .expect("the numbers in magic_numbers.rs are valid magics")
    })
}

// ANCHOR: lookup
/// The squares a rook on `square` attacks, given the occupied squares.
pub fn rook_attacks(square: Square, occupied: Bitboard) -> Bitboard {
    let tables = tables();
    tables.attacks[tables.rook[square.index()].index(occupied)]
}

/// The squares a bishop on `square` attacks, given the occupied squares.
pub fn bishop_attacks(square: Square, occupied: Bitboard) -> Bitboard {
    let tables = tables();
    tables.attacks[tables.bishop[square.index()].index(occupied)]
}
// ANCHOR_END: lookup

/// The rook magic for `square` (for the shell's `magic` command and the book).
pub fn rook_magic(square: Square) -> Magic {
    tables().rook[square.index()]
}

/// The bishop magic for `square`.
pub fn bishop_magic(square: Square) -> Magic {
    tables().bishop[square.index()]
}

/// The relevant-occupancy mask: the slider's empty-board attacks without the edge squares
/// that don't matter. A square on an edge file or rank is still relevant if the slider stands
/// on that same edge and moves along it.
fn relevant_mask(square: Square, empty_board_attacks: Bitboard) -> Bitboard {
    let edge_files =
        (Bitboard::file(File::A) | Bitboard::file(File::H)) & !Bitboard::file(square.file());
    let edge_ranks = (Bitboard::rank(Rank::First) | Bitboard::rank(Rank::Eighth))
        & !Bitboard::rank(square.rank());
    empty_board_attacks & !edge_files & !edge_ranks
}

/// Every subset of `mask`, with the slider's attacks for it.
///
/// The "Carry-Rippler" trick, `subset = (subset - mask) & mask`, steps through all subsets of
/// `mask`, ending back at zero.
fn occupancies(square: Square, mask: Bitboard, reference: Reference) -> Vec<(Bitboard, Bitboard)> {
    let mut result = Vec::with_capacity(1 << mask.count());
    let mut subset = 0u64;
    loop {
        result.push((Bitboard(subset), reference(square, Bitboard(subset))));
        subset = subset.wrapping_sub(mask.0) & mask.0;
        if subset == 0 {
            break;
        }
    }
    result
}

/// Fills `table` (this square's part of the attack table) for `magic`. Returns false if two
/// occupancies that need different attacks land on the same index.
fn fill_table(
    magic: &Magic,
    occupancies: &[(Bitboard, Bitboard)],
    table: &mut [Option<Bitboard>],
) -> bool {
    table.iter_mut().for_each(|entry| *entry = None);
    occupancies.iter().all(|&(occupied, result)| {
        let slot = &mut table[magic.index(occupied) - magic.offset];
        match slot {
            None => {
                *slot = Some(result);
                true
            }
            // Same attacks: a harmless collision. Different attacks: this magic fails.
            Some(existing) => *existing == result,
        }
    })
}

/// A small, fast pseudo-random generator (xorshift64*). Seeded, so a search finds the same
/// magics every time.
pub struct Rng(pub u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A random number with few bits set: such numbers make good magic candidates.
    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

// ANCHOR: find_magic
/// Searches for a magic number for `square`: random sparse candidates until one maps every
/// relevant occupancy to an index without a harmful collision.
pub fn find_magic(square: Square, reference: Reference, rng: &mut Rng) -> u64 {
    let mask = relevant_mask(square, reference(square, Bitboard::EMPTY));
    let occupancies = occupancies(square, mask, reference);
    let mut table = vec![None; occupancies.len()];
    loop {
        let magic = rng.sparse();
        // Quick filter: a good magic moves enough mask bits into the top byte.
        if (mask.0.wrapping_mul(magic) >> 56).count_ones() < 6 {
            continue;
        }
        let candidate = Magic {
            mask,
            magic,
            shift: 64 - mask.count(),
            offset: 0,
        };
        if fill_table(&candidate, &occupancies, &mut table) {
            return magic;
        }
    }
}
// ANCHOR_END: find_magic

/// Finds magic numbers for every square: `(rook magics, bishop magics)`.
///
/// This is what the `find-magics` tool runs to produce `magic_numbers.rs`.
pub fn find_all_magics(seed: u64) -> ([u64; 64], [u64; 64]) {
    let mut rng = Rng(seed);
    let mut rook = [0; 64];
    let mut bishop = [0; 64];
    for square in Square::all() {
        rook[square.index()] = find_magic(square, rook_attacks_by_rays, &mut rng);
        bishop[square.index()] = find_magic(square, bishop_attacks_by_rays, &mut rng);
    }
    (rook, bishop)
}

/// Builds every square's magic and the shared attack table from known magic numbers.
/// Returns `None` if any number doesn't work for its square.
fn build_tables(rook_magics: &[u64; 64], bishop_magics: &[u64; 64]) -> Option<MagicTables> {
    let mut attacks = Vec::new();
    let mut add = |square: Square, magic_number: u64, reference: Reference| {
        let mask = relevant_mask(square, reference(square, Bitboard::EMPTY));
        let magic = Magic {
            mask,
            magic: magic_number,
            shift: 64 - mask.count(),
            offset: attacks.len(),
        };
        let occupancies = occupancies(square, mask, reference);
        let mut table = vec![None; occupancies.len()];
        if !fill_table(&magic, &occupancies, &mut table) {
            return None;
        }
        attacks.extend(table.iter().map(|entry| entry.unwrap_or(Bitboard::EMPTY)));
        Some(magic)
    };
    let empty = Magic {
        mask: Bitboard::EMPTY,
        magic: 0,
        shift: 0,
        offset: 0,
    };
    let mut rook = [empty; 64];
    let mut bishop = [empty; 64];
    for square in Square::all() {
        rook[square.index()] = add(square, rook_magics[square.index()], rook_attacks_by_rays)?;
        bishop[square.index()] = add(
            square,
            bishop_magics[square.index()],
            bishop_attacks_by_rays,
        )?;
    }
    Some(MagicTables {
        rook,
        bishop,
        attacks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks magic lookups against ray walking for every relevant occupancy of every square.
    fn check_all(magic: fn(Square) -> Magic, lookup: Reference, reference: Reference) {
        for square in Square::all() {
            let mask = magic(square).mask.0;
            let mut subset = 0u64;
            loop {
                // Add some irrelevant pieces too: they must not change the answer.
                let occupied = Bitboard(subset | (!mask & 0x8100_0000_0000_0081));
                assert_eq!(
                    lookup(square, occupied),
                    reference(square, occupied),
                    "{square} with {occupied:?}"
                );
                subset = subset.wrapping_sub(mask) & mask;
                if subset == 0 {
                    break;
                }
            }
        }
    }

    #[test]
    fn rook_magics_match_ray_walking() {
        check_all(rook_magic, rook_attacks, rook_attacks_by_rays);
    }

    #[test]
    fn bishop_magics_match_ray_walking() {
        check_all(bishop_magic, bishop_attacks, bishop_attacks_by_rays);
    }

    #[test]
    fn masks_have_the_expected_sizes() {
        assert_eq!(rook_magic(Square::A1).mask.count(), 12);
        assert_eq!(rook_magic(Square::D4).mask.count(), 10);
        assert_eq!(bishop_magic(Square::D4).mask.count(), 9);
        assert_eq!(bishop_magic(Square::A1).mask.count(), 6);
        // The edge square at the end of a ray is never relevant...
        assert!(!rook_magic(Square::D4).mask.contains(Square::D8));
        // ...but a rook on an edge still cares about squares along that edge.
        assert!(rook_magic(Square::A1).mask.contains(Square::A7));
    }

    #[test]
    fn the_search_finds_working_magics() {
        let mut rng = Rng(1);
        let mut rooks = magic_numbers::ROOK_MAGICS;
        let mut bishops = magic_numbers::BISHOP_MAGICS;
        for square in [Square::A1, Square::E4] {
            rooks[square.index()] = find_magic(square, rook_attacks_by_rays, &mut rng);
            bishops[square.index()] = find_magic(square, bishop_attacks_by_rays, &mut rng);
        }
        assert!(build_tables(&rooks, &bishops).is_some());
    }

    #[test]
    fn a_wrong_magic_is_rejected() {
        let mut rooks = magic_numbers::ROOK_MAGICS;
        rooks[0] = 1;
        assert!(build_tables(&rooks, &magic_numbers::BISHOP_MAGICS).is_none());
    }

    #[test]
    fn table_size() {
        // 102,400 rook entries plus 5,248 bishop entries.
        assert_eq!(tables().attacks.len(), 102_400 + 5_248);
    }
}
