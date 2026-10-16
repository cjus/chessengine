# Buckets: one cache line per key

*Milestone M5, after stage 20 · on `main`, no stage tag · code in `crates/engine/src/tt.rs`*

Chapter 8's table gives every key one slot. Two positions whose keys pick the same slot fight
over it, and the replacement rule decides which result is lost. That costs most when the loser
was a deep search: a result from depth 12 can be thrown away to make room for one from depth 2,
just because it came from the previous move.

**Buckets** give each key several slots to choose from. The number of slots isn't arbitrary: it
comes from how the processor reads memory.

Like chapter 17's threads, this is about the machine the engine runs on, not chess. It came after
stage 20, as strength work towards version 1.0, so no stage tag contains it. Switch it with
`set ttbuckets on|off`.

**cb2026 ships with it off.** In games it made the engine weaker, about 9 Elo at 8+0.08 (see
"What it's worth"). The table really does keep more of what it is given, as this chapter shows;
what that bought in the games was a bigger search, not a better one.

## Cache lines

A table of 16 MB doesn't fit in the processor's caches, so most probes wait for main memory,
and memory doesn't arrive a byte at a time. It comes in **cache lines**, blocks of a fixed size
at aligned addresses. Asking for one byte fetches the whole line around it.

Apple Silicon's lines are 128 bytes (`sysctl hw.cachelinesize` prints 128 on an M1 Max). Most
x86 processors use 64. A slot is 16 bytes, the two 64-bit words of chapter 17's lock-free
entries. So in chapter 8's table, a probe that misses the caches fetches eight slots and looks at
one of them.

A bucket uses all eight:

```rust,ignore
{{#include ../../crates/engine/src/tt.rs:bucket}}
```

`repr(align(128))` makes every bucket start on a line boundary, so a bucket is exactly one line,
never two halves of two lines. A `Vec` of buckets is aligned the same way by the allocator, so
this needs no `unsafe` code. A compile-time assertion checks the size and alignment.

## Finding a position

A key now picks a bucket instead of a slot, with the same multiplication as chapter 8:

```rust,ignore
{{#include ../../crates/engine/src/tt.rs:slots_for}}
```

A probe reads the bucket's slots in turn until one has the key. A miss reads all eight, but the
first read already brought the whole line into the cache, so the other seven cost a few
instructions each, not another wait for memory. `bench` measured the same speed both ways:
about 1.63 million nodes per second over five runs each on the M1 Max.

Take a key as a fraction *x* of 2^64 and a table of *B* buckets. Its bucket is `⌊Bx⌋`, and
with buckets off its slot is `⌊8Bx⌋`, since there are eight times as many slots. Slot `s` is in
bucket `⌊s / 8⌋`, and `⌊⌊8Bx⌋ / 8⌋ = ⌊Bx⌋`, so a key's single slot is always one of the slots of
its bucket. Switching buckets on in the middle of a game still finds every entry. Switching them
off only finds the entries that happen to sit in their key's own slot.

## Making room

A store goes to the slot that already holds the position, or else an empty slot. When all eight
slots hold other positions, it goes to the slot whose entry is **worth least**:

```rust,ignore
{{#include ../../crates/engine/src/tt.rs:store}}
```

An entry's worth is its depth, less 8 plies for every search since it was stored. Deep results
cost the most to search again, and old ones are the least likely to be needed. Take a depth-14
entry from the previous move and a depth-2 entry from this one: they are worth 14 − 8 = 6 and 2,
so the shallow one goes. Two moves later the old entry is worth 14 − 16 = −2, and it goes
first.

Chapter 8's rule then decides whether the chosen entry is replaced. It always is unless it comes
from this search and is deeper than the new result. In a bucket the chosen entry is the least
valuable of eight, so that is rare.

The age weighting is Stockfish's (GPL-3.0), where an entry one search old also counts as 8 plies
shallower. Stockfish packs three 10-byte entries into 32 bytes; cb2026 keeps its 16-byte slots
with full keys and puts eight in a line. The code is cb2026's own.

## What it changes

The six positions of chapters 10 to 14, each searched from an empty table:

| Table | Buckets | Depth 12 nodes | Depth 15 nodes |
| --- | --- | ---: | ---: |
| 1 MB | off | 1,670,589 | 5,123,837 |
| 1 MB | on | 1,583,256 (−5%) | 4,900,688 (−4%) |
| 16 MB | off | 1,537,930 | 4,979,436 |
| 16 MB | on | 1,583,673 (+3%) | 5,318,128 (+7%) |

A 1 MB table has 65,536 slots, fewer than these searches have results to store, so it is under
pressure, and buckets shrink the tree. At depth 12, with buckets, the 1 MB table does almost
exactly what the 16 MB one does: the share of nodes that find an entry is the same to a tenth of
a percent in every position, and the trees differ by 417 nodes.

At 16 MB the tree gets *bigger*. The table keeps more deep entries, and every singular
extension (chapter 14.1) starts from a deep enough entry. At depth 15, with buckets on, the six
positions run 9% more singular searches (49,042 against 44,877) and extend 12% more hash moves.
More extensions mean a bigger tree. As chapter 10.1 found, node counts can't say whether that
makes the engine stronger. Only games can, and here they said no: about **−9 Elo** at 8+0.08.
The likeliest reason is the one above. At 16 MB and this time control the table is under little
pressure, so buckets save few results that matter, and the extra deep entries start extensions
that cost more time than they repay. That is a reading of the node counts, not something the
games showed; a test at a longer time control, where the table fills, would say more.

With `set ttbuckets on`, `bench` counts 1,114,877 nodes instead of 1,098,926 (+1.5%). Off, the
default, the table behaves exactly as in chapter 8, and `bench` is 1,098,926.

## Pitfalls

- **Other processors.** With 64-byte lines, a bucket spans two. They are next to each other, and
  many processors fetch such pairs together (Intel's "adjacent line" prefetcher does). The
  bucket size is the same everywhere so that `bench`, which identifies a version of the engine,
  counts the same nodes on every machine.
- **Alignment.** `repr(align(...))` takes a number, not a constant, so the 128 is written twice.
  The compile-time assertion catches a mismatch.
- **Nodes aren't Elo.** See above: a better table made the 16 MB tree bigger.
- **Light pressure in short games.** At 8+0.08 a move searches a few hundred thousand nodes,
  and the default table has about a million slots. Buckets matter more when the table fills:
  longer time controls, more threads, or a smaller `Hash`.

## What it's worth

| Test | Result |
| --- | --- |
| Buckets vs `main` without them, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **−8.8 ± 8.6 Elo** over 2,685 games (+536 −604 =1545): failed (LLR −2.96); off by default |

## Further reading

- Chess Programming Wiki: [Transposition Table](https://www.chessprogramming.org/Transposition_Table),
  [Replacement Strategies](https://www.chessprogramming.org/Replacement_Strategies).
- Stockfish's source (GPL-3.0), `tt.h` and `tt.cpp`: clusters of three entries and replacement
  by depth minus age.
- Ulrich Drepper, ["What Every Programmer Should Know About Memory"](https://www.akkadia.org/drepper/cpumemory.pdf)
  (2007): cache lines, and why a miss costs what it does.
