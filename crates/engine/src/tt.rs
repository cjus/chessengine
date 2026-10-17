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
//! ## Buckets
//!
//! A table of many megabytes doesn't fit in the CPU's caches, so most probes wait for main
//! memory, and memory is fetched a whole cache line at a time. Apple Silicon's lines are
//! [`CACHE_LINE_BYTES`] (128) bytes and a slot is 16, so a probe of a single slot fetches eight
//! and reads one. With **buckets** (the `tt_buckets` technique, after stage 20), a key picks a
//! bucket of [`BUCKET_SLOTS`] slots that fills exactly one line, and the position may be in any
//! of them: eight places for the price of one fetch.
//!
//! A store goes to the slot that already holds the position, else to an empty one, else to
//! the one whose entry is worth least: the shallowest, with each search since it was stored
//! counting as [`AGE_PLIES`] plies less depth. The rule above then decides whether to replace
//! it. With one slot per key, a deep result and a fresh one compete for that slot and one of
//! them is lost; a bucket can keep both. Switched off, a key picks a single slot, as in stage 8.
//!
//! **Buckets are off by default.** In games they measured weaker, −8.8 ± 8.6 Elo over 2,685
//! games at 8+0.08 (cjus/chessengine#86): at 16 MB the table keeps more deep entries, singular
//! extensions start from more of them, and the trees grow. At 40+0.4, where the table fills
//! more, they came out about even: +1.6 ± 6.3 over 4,630 games, stopped with no verdict
//! (cjus/chessengine#92). The switch stays, for tests where the table is under more pressure
//! (a smaller hash, more threads).
//!
//! Pitfalls:
//!
//! - **Other CPUs.** Most x86 and other ARM processors have 64-byte lines, so there a bucket
//!   spans two. They are next to each other, and many processors fetch such pairs together.
//!   The bucket size is the same everywhere so that `bench` counts the same nodes on every
//!   machine.
//! - **Probing costs more work.** A miss reads all eight slots instead of one. They are already
//!   in the cache by then, so it costs a few instructions, not another wait for memory.
//! - **Switching mid-game.** A key's single slot lies inside its bucket (see
//!   `TranspositionTable::slots_for`), so switching buckets on still finds every entry. Switching
//!   them off finds only the entries that happen to sit in their key's own slot.
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
/// The size of one of Apple Silicon's cache lines, in bytes: the unit memory is fetched in, and
/// the size of a bucket.
pub const CACHE_LINE_BYTES: usize = 128;
/// The slots in a bucket: as many as fill one cache line.
pub const BUCKET_SLOTS: usize = CACHE_LINE_BYTES / std::mem::size_of::<Slot>();
/// Choosing which entry of a full bucket to replace, each search since an entry was stored
/// counts as this many plies less depth.
pub const AGE_PLIES: i32 = 8;

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

impl Slot {
    /// The slot's entry if it is for `key`. A torn entry looks like another key's, so it
    /// isn't returned.
    fn entry_for(&self, key: u64) -> Option<Entry> {
        let data = self.data.load(Ordering::Relaxed);
        if self.key_xor_data.load(Ordering::Relaxed) ^ data != key {
            return None;
        }
        unpack(key, data)
    }

    /// Whatever the slot holds, if anything.
    fn entry(&self) -> Option<Entry> {
        let data = self.data.load(Ordering::Relaxed);
        let key = self.key_xor_data.load(Ordering::Relaxed) ^ data;
        unpack(key, data)
    }

    /// Writes an entry (data from [`pack`]); 0 and 0 empty the slot.
    fn write(&self, key: u64, data: u64) {
        self.data.store(data, Ordering::Relaxed);
        self.key_xor_data.store(key ^ data, Ordering::Relaxed);
    }
}

impl Clone for Slot {
    /// A copy of what the slot holds now (atomics can't simply be copied).
    fn clone(&self) -> Slot {
        Slot {
            key_xor_data: AtomicU64::new(self.key_xor_data.load(Ordering::Relaxed)),
            data: AtomicU64::new(self.data.load(Ordering::Relaxed)),
        }
    }
}

// ANCHOR: bucket
/// [`BUCKET_SLOTS`] slots, aligned so that they fill exactly one cache line. (`align` only
/// takes a number, not a constant; the assertion below checks it matches.)
#[derive(Clone, Debug, Default)]
#[repr(align(128))]
struct Bucket([Slot; BUCKET_SLOTS]);
// ANCHOR_END: bucket

const _: () = assert!(
    std::mem::size_of::<Bucket>() == CACHE_LINE_BYTES
        && std::mem::align_of::<Bucket>() == CACHE_LINE_BYTES
);

/// The transposition table. It is shared between search threads by reference: all its
/// methods but [`TranspositionTable::set_buckets`] take `&self`, and every slot is made of
/// atomics.
pub struct TranspositionTable {
    buckets: Vec<Bucket>,
    /// Whether a key picks a whole bucket (`true`) or a single slot.
    use_buckets: bool,
    generation: AtomicU8,
}

impl Clone for TranspositionTable {
    fn clone(&self) -> TranspositionTable {
        TranspositionTable {
            buckets: self.buckets.clone(),
            use_buckets: self.use_buckets,
            generation: AtomicU8::new(self.generation.load(Ordering::Relaxed)),
        }
    }
}

impl std::fmt::Debug for TranspositionTable {
    /// A summary: printing millions of slots helps nobody.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranspositionTable")
            .field("slots", &self.len())
            .field("use_buckets", &self.use_buckets)
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
    /// A table using about `megabytes` of memory (at least one bucket), with buckets off. The
    /// engine sets them from the `tt_buckets` technique at the start of each search.
    pub fn new(megabytes: usize) -> TranspositionTable {
        let count = (megabytes * 1024 * 1024 / std::mem::size_of::<Bucket>()).max(1);
        TranspositionTable {
            buckets: (0..count).map(|_| Bucket::default()).collect(),
            use_buckets: false,
            generation: AtomicU8::new(0),
        }
    }

    /// The table's size in megabytes, rounded to the nearest.
    pub fn megabytes(&self) -> usize {
        let bytes = self.buckets.len() * std::mem::size_of::<Bucket>();
        (bytes + 512 * 1024) / (1024 * 1024)
    }

    /// The number of slots.
    pub fn len(&self) -> usize {
        self.buckets.len() * BUCKET_SLOTS
    }

    /// Whether the table has no slots (never true: it always has at least one bucket).
    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// Every slot, in order.
    fn slots(&self) -> impl Iterator<Item = &Slot> {
        self.buckets.iter().flat_map(|bucket| &bucket.0)
    }

    /// Switches buckets on or off (the `tt_buckets` technique). The entries stay where they
    /// are; see the module docs for what each direction still finds.
    pub fn set_buckets(&mut self, on: bool) {
        self.use_buckets = on;
    }

    /// Empties the table (a new game, or the UCI `Clear Hash` button).
    pub fn clear(&self) {
        for slot in self.slots() {
            slot.write(0, 0);
        }
        self.generation.store(0, Ordering::Relaxed);
    }

    /// Marks the start of a new search, so older entries are replaced first.
    pub fn new_search(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    // ANCHOR: slots_for
    /// The slots where the position with `key` may be stored: its bucket, or with buckets off
    /// a single slot.
    ///
    /// Taking the key modulo the table size would work, but division is slow. Instead the key
    /// is treated as a fraction of 2^64 and scaled to the number of buckets (or slots):
    /// `(key × count) >> 64`. It is one multiplication, works for any table size, and spreads
    /// keys evenly because Zobrist keys are uniformly random. (Daniel Lemire, "A fast
    /// alternative to the modulo reduction", 2016.) There are [`BUCKET_SLOTS`] times as many
    /// slots as buckets, so a key's slot is always one of the slots of its bucket.
    fn slots_for(&self, key: u64) -> &[Slot] {
        let scale = |count: usize| ((key as u128 * count as u128) >> 64) as usize;
        if self.use_buckets {
            &self.buckets[scale(self.buckets.len())].0
        } else {
            let index = scale(self.len());
            std::slice::from_ref(&self.buckets[index / BUCKET_SLOTS].0[index % BUCKET_SLOTS])
        }
    }
    // ANCHOR_END: slots_for

    /// The entry stored for `key`, if there is one (and it wasn't torn by a concurrent write).
    pub fn probe(&self, key: u64) -> Option<Entry> {
        self.slots_for(key)
            .iter()
            .find_map(|slot| slot.entry_for(key))
    }

    // ANCHOR: store
    /// Stores a search result for the position with `key`.
    pub fn store(&self, key: u64, best_move: Option<Move>, score: i32, depth: u32, bound: Bound) {
        let generation = self.generation.load(Ordering::Relaxed);
        let depth = depth.min(u8::MAX as u32) as u8;
        let slot = self.slot_to_store(key, generation);
        let old = slot.entry();
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
            slot.write(key, pack(best_move, score, depth, bound, generation));
        }
    }

    /// Where a result for `key` goes: the slot that already holds the position, else an empty
    /// one, else the one whose entry is worth least (see [`worth`]). With buckets off there is
    /// only one candidate.
    fn slot_to_store(&self, key: u64, generation: u8) -> &Slot {
        let slots = self.slots_for(key);
        slots
            .iter()
            .find(|slot| slot.entry_for(key).is_some())
            .unwrap_or_else(|| {
                // `min_by_key` returns the first of equals, so the first empty slot.
                slots
                    .iter()
                    .min_by_key(|slot| slot.entry().map_or(i32::MIN, |e| worth(e, generation)))
                    .expect("a key always has at least one slot")
            })
    }
    // ANCHOR_END: store

    /// How full the table is, in permille, estimated from the first thousand slots (UCI
    /// `info hashfull`). Only entries from the current search count.
    pub fn hashfull(&self) -> u32 {
        let generation = self.generation.load(Ordering::Relaxed);
        let sample = self.len().min(1000);
        let used = self
            .slots()
            .take(sample)
            .filter(|slot| {
                unpack(0, slot.data.load(Ordering::Relaxed))
                    .is_some_and(|e| e.generation == generation)
            })
            .count();
        (used * 1000 / sample.max(1)) as u32
    }
}

/// How much an entry is worth keeping when a full bucket needs room: its depth, less
/// [`AGE_PLIES`] for each search since it was stored. Deep results cost the most to search
/// again; old ones are the least likely to be needed. The weighting is Stockfish's (GPL-3.0),
/// where an entry one search old also counts as 8 plies shallower; the code is cb2026's own.
fn worth(entry: Entry, generation: u8) -> i32 {
    let age = generation.wrapping_sub(entry.generation);
    i32::from(entry.depth) - AGE_PLIES * i32::from(age)
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
        // One bucket and buckets off: small keys all land in the first slot.
        let mut tt = TranspositionTable::new(0);
        tt.set_buckets(false);
        assert_eq!(tt.len(), BUCKET_SLOTS);
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
    fn a_key_has_a_bucket_or_one_slot_for_any_size() {
        for mb in [0, 1, 3] {
            let mut tt = TranspositionTable::new(mb);
            tt.set_buckets(true);
            assert_eq!(tt.megabytes(), mb);
            for key in [0, 1, u64::MAX, 0x8000_0000_0000_0000] {
                // Indexing out of range would panic.
                assert_eq!(tt.slots_for(key).len(), BUCKET_SLOTS);
                tt.set_buckets(false);
                assert_eq!(tt.slots_for(key).len(), 1);
                tt.set_buckets(true);
            }
        }
    }

    #[test]
    fn a_keys_slot_is_inside_its_bucket() {
        let mut tt = TranspositionTable::new(1);
        tt.set_buckets(true);
        for key in [0, 1, 0x1234_5678_9abc_def0, u64::MAX, 0x8000_0000_0000_0000] {
            let bucket = tt.slots_for(key).as_ptr_range();
            tt.set_buckets(false);
            assert!(bucket.contains(&tt.slots_for(key).as_ptr()));
            tt.set_buckets(true);
        }
    }

    #[test]
    fn buckets_are_one_cache_line() {
        assert_eq!(BUCKET_SLOTS, 8);
        let tt = TranspositionTable::new(1);
        let address = tt.buckets.as_ptr() as usize;
        assert_eq!(address % CACHE_LINE_BYTES, 0);
    }

    #[test]
    fn a_bucket_holds_positions_that_one_slot_cannot() {
        // One bucket: small keys all share the first slot with buckets off.
        let mut tt = TranspositionTable::new(0);
        tt.set_buckets(false);
        tt.store(1, None, 10, 5, Bound::Exact);
        tt.store(2, None, 20, 5, Bound::Exact);
        assert_eq!(tt.probe(1), None);
        tt.set_buckets(true);
        tt.clear();
        for key in 1..=8 {
            tt.store(key, None, key as i32, 5, Bound::Exact);
        }
        for key in 1..=8 {
            assert_eq!(tt.probe(key).unwrap().score, key as i32);
        }
    }

    #[test]
    fn a_full_bucket_replaces_its_least_valuable_entry() {
        let mut tt = TranspositionTable::new(0);
        tt.set_buckets(true);
        // Keys 1 to 8 searched to depths 10 to 17.
        for key in 1..=8 {
            tt.store(key, None, 0, 9 + key as u32, Bound::Exact);
        }
        // Too shallow to replace even the shallowest entry from this search.
        tt.store(9, None, 0, 3, Bound::Exact);
        assert_eq!(tt.probe(9), None);
        // As deep as the shallowest: it goes.
        tt.store(9, None, 0, 10, Bound::Exact);
        assert_eq!(tt.probe(1), None);
        assert!((2..=9).all(|key| tt.probe(key).is_some()));
        // A position already in the bucket is updated in its own slot.
        tt.store(5, None, 55, 1, Bound::Upper);
        assert_eq!(tt.probe(5).unwrap().score, 55);
        assert!((2..=9).all(|key| tt.probe(key).is_some()));
    }

    #[test]
    fn an_old_deep_entry_goes_before_a_fresh_shallow_one() {
        let mut tt = TranspositionTable::new(0);
        tt.set_buckets(true);
        tt.store(1, None, 0, 9, Bound::Exact);
        tt.new_search();
        tt.new_search();
        for key in 2..=8 {
            tt.store(key, None, 0, 2, Bound::Exact);
        }
        // Key 1 is worth 9 - 2 × 8 = -7, the others 2.
        tt.store(9, None, 0, 2, Bound::Exact);
        assert_eq!(tt.probe(1), None);
        assert!((2..=9).all(|key| tt.probe(key).is_some()));
    }

    #[test]
    fn hashfull_counts_this_search_only() {
        let tt = TranspositionTable::new(0);
        assert_eq!(tt.hashfull(), 0);
        tt.store(5, None, 0, 1, Bound::Exact);
        assert_eq!(tt.hashfull(), 1000 / BUCKET_SLOTS as u32);
        tt.new_search();
        assert_eq!(tt.hashfull(), 0);
    }
}
