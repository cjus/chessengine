//! The transposition table: remembering positions the search has already seen.
//!
//! Different move orders often reach the same position: 1. e4 e5 2. Nf3 and 1. Nf3 e5 2. e4
//! are a **transposition**. A plain alpha-beta search would search that position once for
//! each path. The transposition table (TT) is a large hash table, indexed by the position's
//! Zobrist key, that stores what the search learned about each position:
//!
//! - the **best move** found there, which is searched first next time, even when the stored
//!   result is too shallow to reuse. With iterative deepening this matters most: depth *d*
//!   leaves behind a best move for nearly every position depth *d + 1* visits;
//! - the **score**, the **depth** it was searched to, and what kind of score it is (the
//!   [`Bound`]). A result searched at least as deep as needed can end the search of the
//!   position at once (a **TT cutoff**).
//!
//! ## Bounds
//!
//! Alpha-beta rarely knows a position's exact score. A move that fails high (`>= beta`) only
//! proves the score is *at least* that much; a position where every move fails low only
//! proves it is *at most* alpha. So each entry says which:
//!
//! | Bound | Meaning | Usable when |
//! | --- | --- | --- |
//! | `Exact` | the score | always |
//! | `Lower` | score ≥ stored | stored ≥ beta (still fails high) |
//! | `Upper` | score ≤ stored | stored ≤ alpha (still fails low) |
//!
//! ## Mate scores
//!
//! Mate scores count plies from the root ("mate in 5 from here"), but the same position can be
//! reached at different distances from the root. Entries store mate scores relative to the
//! position itself and convert back when read ([`score_to_tt`], [`score_from_tt`]).
//!
//! ## Size and replacement
//!
//! The table has a fixed size (the UCI `Hash` option, in megabytes) and is always full of
//! something. When a new result maps to a slot that is already taken, it replaces the old one
//! if the old one is from an earlier search, or was searched less deeply, or is the same
//! position. Deep results are expensive, so they're kept until something deeper or newer comes
//! along.
//!
//! Each slot keeps the full 64-bit key, so two positions that share a slot are told apart.
//! From stage 17 the slots are atomics shared by all search threads (see `Slot`).
//! Two different positions with the *same* key are possible in theory, but with 64-bit keys
//! the chance is negligible; the stored move is still checked for legality before use.
//!
//! ## Pitfalls
//!
//! - **Repetitions.** The TT doesn't know how a position was reached, so a draw by repetition
//!   found on one path can be reused on another. All engines accept this small error (the
//!   "graph history interaction" problem).
//! - **Aborted searches.** A search stopped by the clock returns garbage, which must never be
//!   stored.
//!
//! See the Chess Programming Wiki, [Transposition Table](https://www.chessprogramming.org/Transposition_Table)
//! and [Replacement Strategies](https://www.chessprogramming.org/Replacement_Strategies).

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use cb2026_chess::Move;

use crate::search::MAX_PLY;
use crate::tablebase::TB_WIN;

/// The table size when nothing else is asked for, in megabytes.
pub const DEFAULT_HASH_MB: usize = 16;
/// The largest table the UCI `Hash` option allows, in megabytes.
pub const MAX_HASH_MB: usize = 65_536;

// ANCHOR: entry
/// What a stored score means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    /// The exact score.
    Exact,
    /// A lower bound: the real score is at least this (the search failed high).
    Lower,
    /// An upper bound: the real score is at most this (the search failed low).
    Upper,
}

/// One stored result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The position's Zobrist key.
    pub key: u64,
    /// The best move found, if any (a fail-low node may have none).
    pub best_move: Option<Move>,
    /// The score, with mates relative to this position (see [`score_to_tt`]).
    pub score: i32,
    /// The depth the position was searched to.
    pub depth: u8,
    /// What kind of score it is.
    pub bound: Bound,
    /// The search that stored it, for replacement.
    pub generation: u8,
}
// ANCHOR_END: entry

// ANCHOR: slot
/// One slot of the table: an entry packed into two 64-bit words that threads can read and
/// write without locks.
///
/// With several search threads (Lazy SMP, stage 17), two threads can write the same slot at
/// once, and a reader can see half of one write and half of the other: a "torn" entry. The
/// trick, from Robert Hyatt and Timothy Mann's "A lock-less transposition table
/// implementation for parallel search chess engines" (ICGA Journal, 2002): store the key
/// XORed with the data. A reader XORs the two words it loaded; it only gets the right key back
/// if both words come from the same write, so a torn entry simply looks like a miss.
#[derive(Debug, Default)]
struct Slot {
    key_xor_data: AtomicU64,
    data: AtomicU64,
}
// ANCHOR_END: slot

/// The transposition table. It is shared between search threads by reference: all its
/// methods take `&self`, and every slot is made of atomics.
pub struct TranspositionTable {
    slots: Vec<Slot>,
    generation: AtomicU8,
}

impl Clone for TranspositionTable {
    fn clone(&self) -> TranspositionTable {
        TranspositionTable {
            slots: self
                .slots
                .iter()
                .map(|slot| Slot {
                    key_xor_data: AtomicU64::new(slot.key_xor_data.load(Ordering::Relaxed)),
                    data: AtomicU64::new(slot.data.load(Ordering::Relaxed)),
                })
                .collect(),
            generation: AtomicU8::new(self.generation.load(Ordering::Relaxed)),
        }
    }
}

impl std::fmt::Debug for TranspositionTable {
    /// A summary: printing millions of slots helps nobody.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranspositionTable")
            .field("slots", &self.slots.len())
            .field("generation", &self.generation.load(Ordering::Relaxed))
            .finish()
    }
}

impl Default for TranspositionTable {
    fn default() -> TranspositionTable {
        TranspositionTable::new(DEFAULT_HASH_MB)
    }
}

// ANCHOR: pack
/// Packs everything but the key into one word:
/// move (16 bits) | score (16) | depth (8) | bound (2) | generation (8).
/// A bound of 0 marks an empty slot.
fn pack(best_move: Option<Move>, score: i32, depth: u8, bound: Bound, generation: u8) -> u64 {
    let bound_bits: u64 = match bound {
        Bound::Exact => 1,
        Bound::Lower => 2,
        Bound::Upper => 3,
    };
    u64::from(best_move.map_or(0, Move::to_bits))
        | u64::from(score as i16 as u16) << 16
        | u64::from(depth) << 32
        | bound_bits << 40
        | u64::from(generation) << 42
}

/// Unpacks a word made by [`pack`] (`None` for an empty slot).
fn unpack(key: u64, data: u64) -> Option<Entry> {
    let bound = match (data >> 40) & 3 {
        1 => Bound::Exact,
        2 => Bound::Lower,
        3 => Bound::Upper,
        _ => return None,
    };
    let move_bits = data as u16;
    Some(Entry {
        key,
        // Move 0 (a1 to a1) is never legal, so it stands for "no move".
        best_move: (move_bits != 0).then(|| Move::from_bits(move_bits)),
        score: i32::from((data >> 16) as u16 as i16),
        depth: (data >> 32) as u8,
        bound,
        generation: (data >> 42) as u8,
    })
}
// ANCHOR_END: pack

impl TranspositionTable {
    /// A table using about `megabytes` of memory (at least one slot).
    pub fn new(megabytes: usize) -> TranspositionTable {
        let slot_size = std::mem::size_of::<Slot>();
        let count = (megabytes * 1024 * 1024 / slot_size).max(1);
        TranspositionTable {
            slots: (0..count).map(|_| Slot::default()).collect(),
            generation: AtomicU8::new(0),
        }
    }

    /// The table's size in megabytes, rounded to the nearest.
    pub fn megabytes(&self) -> usize {
        let bytes = self.slots.len() * std::mem::size_of::<Slot>();
        (bytes + 512 * 1024) / (1024 * 1024)
    }

    /// The number of slots.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the table has no slots (never true: it always has at least one).
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Empties the table (a new game, or the UCI `Clear Hash` button).
    pub fn clear(&self) {
        for slot in &self.slots {
            slot.key_xor_data.store(0, Ordering::Relaxed);
            slot.data.store(0, Ordering::Relaxed);
        }
        self.generation.store(0, Ordering::Relaxed);
    }

    /// Marks the start of a new search, so older entries are replaced first.
    pub fn new_search(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// The slot for `key`.
    ///
    /// Taking the key modulo the table size would work, but division is slow. Instead the key
    /// is treated as a fraction of 2^64 and scaled to the table size: `(key × len) >> 64`.
    /// It is one multiplication, works for any table size, and spreads keys evenly because
    /// Zobrist keys are uniformly random. (Daniel Lemire, "A fast alternative to the modulo
    /// reduction", 2016.)
    fn index(&self, key: u64) -> usize {
        ((key as u128 * self.slots.len() as u128) >> 64) as usize
    }

    /// The entry stored for `key`, if there is one (and it wasn't torn by a concurrent write).
    pub fn probe(&self, key: u64) -> Option<Entry> {
        let slot = &self.slots[self.index(key)];
        let data = slot.data.load(Ordering::Relaxed);
        let key_xor_data = slot.key_xor_data.load(Ordering::Relaxed);
        if key_xor_data ^ data != key {
            return None;
        }
        unpack(key, data)
    }

    // ANCHOR: store
    /// Stores a search result for the position with `key`.
    pub fn store(&self, key: u64, best_move: Option<Move>, score: i32, depth: u32, bound: Bound) {
        let slot = &self.slots[self.index(key)];
        let generation = self.generation.load(Ordering::Relaxed);
        let depth = depth.min(u8::MAX as u32) as u8;
        let old_data = slot.data.load(Ordering::Relaxed);
        let old_key = slot.key_xor_data.load(Ordering::Relaxed) ^ old_data;
        let old = unpack(old_key, old_data);
        let replace = match old {
            None => true,
            Some(old) => old.key == key || old.generation != generation || depth >= old.depth,
        };
        if replace {
            // A fail-low result has no best move; keep the one already known for this position.
            let best_move = best_move.or_else(|| {
                old.filter(|old| old.key == key)
                    .and_then(|old| old.best_move)
            });
            let data = pack(best_move, score, depth, bound, generation);
            slot.data.store(data, Ordering::Relaxed);
            slot.key_xor_data.store(key ^ data, Ordering::Relaxed);
        }
    }
    // ANCHOR_END: store

    /// How full the table is, in permille, estimated from the first thousand slots (UCI
    /// `info hashfull`). Only entries from the current search count.
    pub fn hashfull(&self) -> u32 {
        let generation = self.generation.load(Ordering::Relaxed);
        let sample = self.slots.len().min(1000);
        let used = self.slots[..sample]
            .iter()
            .filter(|slot| {
                unpack(0, slot.data.load(Ordering::Relaxed))
                    .is_some_and(|e| e.generation == generation)
            })
            .count();
        (used * 1000 / sample.max(1)) as u32
    }
}

/// Converts a score at `ply` from the root to the form stored in the table: mate scores count
/// plies from the stored position instead of from the root.
pub fn score_to_tt(score: i32, ply: u32) -> i32 {
    if score >= DISTANCE_SCORES {
        score + ply as i32
    } else if score <= -DISTANCE_SCORES {
        score - ply as i32
    } else {
        score
    }
}

/// Converts a stored score back to a score at `ply` from the root.
pub fn score_from_tt(score: i32, ply: u32) -> i32 {
    if score >= DISTANCE_SCORES {
        score - ply as i32
    } else if score <= -DISTANCE_SCORES {
        score + ply as i32
    } else {
        score
    }
}

/// Scores at least this large count distance from the root: mates, and from stage 18
/// tablebase wins (see [`crate::tablebase::TB_WIN`]).
const DISTANCE_SCORES: i32 = TB_WIN - MAX_PLY as i32;

#[cfg(test)]
mod tests {
    use super::*;
    use cb2026_chess::Position;

    #[test]
    fn stores_and_finds_entries() {
        let tt = TranspositionTable::new(1);
        let pos = Position::startpos();
        let mv = pos.parse_uci_move("e2e4").unwrap();
        assert_eq!(tt.probe(pos.hash()), None);
        tt.store(pos.hash(), Some(mv), 35, 6, Bound::Exact);
        let entry = tt.probe(pos.hash()).unwrap();
        assert_eq!(entry.best_move, Some(mv));
        assert_eq!(
            (entry.score, entry.depth, entry.bound),
            (35, 6, Bound::Exact)
        );
        // A different key in the same slot is not mistaken for this one.
        assert_eq!(tt.probe(pos.hash() ^ 1), None);
        tt.clear();
        assert_eq!(tt.probe(pos.hash()), None);
    }

    #[test]
    fn keeps_deeper_results_from_the_same_search() {
        // One slot, so every key collides.
        let tt = TranspositionTable::new(0);
        assert_eq!(tt.len(), 1);
        tt.store(1, None, 10, 8, Bound::Exact);
        tt.store(2, None, 20, 3, Bound::Exact);
        assert_eq!(tt.probe(1).unwrap().score, 10);
        // A newer search replaces it.
        tt.new_search();
        tt.store(2, None, 20, 3, Bound::Exact);
        assert_eq!(tt.probe(2).unwrap().score, 20);
    }

    #[test]
    fn a_fail_low_keeps_the_known_best_move() {
        let tt = TranspositionTable::new(1);
        let pos = Position::startpos();
        let mv = pos.parse_uci_move("d2d4").unwrap();
        tt.store(pos.hash(), Some(mv), 20, 4, Bound::Exact);
        tt.store(pos.hash(), None, -50, 5, Bound::Upper);
        let entry = tt.probe(pos.hash()).unwrap();
        assert_eq!(entry.best_move, Some(mv));
        assert_eq!(entry.bound, Bound::Upper);
    }

    use crate::search::MATE;

    #[test]
    fn tablebase_scores_are_stored_relative_to_the_position_too() {
        let win_at_root = TB_WIN - 9;
        assert_eq!(score_from_tt(score_to_tt(win_at_root, 4), 4), win_at_root);
        assert_eq!(score_to_tt(win_at_root, 4), TB_WIN - 5);
        assert_eq!(score_to_tt(2500, 4), 2500);
    }

    #[test]
    fn mate_scores_are_stored_relative_to_the_position() {
        // Mate in 3 plies seen from 5 plies below the root: MATE - 8 at the root.
        let at_root = MATE - 8;
        let stored = score_to_tt(at_root, 5);
        assert_eq!(stored, MATE - 3);
        // Reached again 2 plies from the root, it's mate in 5 plies from there.
        assert_eq!(score_from_tt(stored, 2), MATE - 5);
        assert_eq!(score_from_tt(score_to_tt(-MATE + 8, 5), 5), -MATE + 8);
        assert_eq!(score_to_tt(150, 7), 150);
    }

    #[test]
    fn index_is_in_range_for_any_size() {
        for mb in [0, 1, 3] {
            let tt = TranspositionTable::new(mb);
            for key in [0, 1, u64::MAX, 0x8000_0000_0000_0000] {
                assert!(tt.index(key) < tt.len());
            }
        }
    }

    #[test]
    fn hashfull_counts_this_search_only() {
        let tt = TranspositionTable::new(0);
        assert_eq!(tt.hashfull(), 0);
        tt.store(5, None, 0, 1, Bound::Exact);
        assert_eq!(tt.hashfull(), 1000);
        tt.new_search();
        assert_eq!(tt.hashfull(), 0);
    }
}
