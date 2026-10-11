# The transposition table

*Stage 8 · tag `stage-08-tt` · code in `crates/engine/src/tt.rs` and `search.rs`*

1. e4 e5 2. Nf3 and 1. Nf3 e5 2. e4 reach the same position by different routes. Chess is
full of these **transpositions**, and a plain alpha-beta search treats each route as a new
position and searches it again. Iterative deepening makes it worse: depth 7 repeats all of
depth 6's work. The **transposition table** (TT) remembers what the search found in every
position it visited, so it can reuse the result or at least start from the best move.

## What gets stored

The TT is one large array of entries indexed by the position's Zobrist key (chapter 4):

```rust,ignore
{{#include ../../crates/engine/src/tt.rs:entry}}
```

Two parts of an entry are useful in different ways:

- **The best move.** Even when the stored result is too shallow to reuse, its best move is
  probably still best, and searching it first gives alpha-beta its early cutoffs (chapter 5).
  With iterative deepening, depth *d* leaves a best move behind for almost every position
  depth *d + 1* visits. Most of the TT's value comes from this.
- **The score**, if it was searched at least as deep as the search now needs. Then the
  position needn't be searched at all: a **TT cutoff**.

## Bounds

Alpha-beta rarely learns a position's exact score. When a move fails high (`≥ beta`), the
search stops early: it only knows the score is *at least* that. When every move fails low, it
only knows the score is *at most* `alpha`. So each entry records which kind of score it holds,
and a later search uses it only when it settles the question that search is asking:

| Bound | What it says | A TT cutoff when |
| --- | --- | --- |
| `Exact` | score = stored | always |
| `Lower` | score ≥ stored | stored ≥ beta: still fails high |
| `Upper` | score ≤ stored | stored ≤ alpha: still fails low |

Here is the probe at the top of negamax, then the store at the end:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:tt_probe}}
```

```rust,ignore
{{#include ../../crates/engine/src/search.rs:tt_store}}
```

The stored move is checked against the legal moves before it is used. A wrong move could
come from two different positions sharing a key, which is very rare with 64-bit keys but not
impossible.

## Mate scores

A mate score counts plies from the root: `MATE − 7` means mate in 7 plies from where the
search started. But the TT stores positions, and the same position can turn up at ply 3 in one
line and ply 5 in another. So mate scores are stored relative to the position itself ("mated
in 2 plies from here") and converted back when read. Forget this, and the engine reports wrong
mate distances or shuffles its pieces instead of mating.

## Indexing and replacement

The table has a fixed size, set with the UCI `Hash` option (16 MB by default) or `set hash` in
the shell. A key picks a slot with one multiplication instead of a division:

```rust,ignore
let index = ((key as u128 * len as u128) >> 64) as usize;
```

This treats the key as a fraction between 0 and 1 and scales it to the table size, which
works for any size. The trick is from Daniel Lemire, "A fast alternative to the modulo
reduction" (2016).

Slots fill up fast, so the table needs a **replacement strategy** for when a new result maps
to an occupied slot:

```rust,ignore
{{#include ../../crates/engine/src/tt.rs:store}}
```

cb2026 replaces an entry if it is for the same position, from an earlier search, or searched
no deeper than the new result. Deep results cost the most to recompute, so they're kept until
they go stale. Stronger engines use buckets of several entries per slot and finer aging. The
Chess Programming Wiki lists the options.

## What it saves

Each pair below has the same move and score, with and without the table (`set tt off`), at
depth 6:

| Position | With TT | Without | Saved |
| --- | ---: | ---: | ---: |
| Kiwipete | 694,370 nodes | 1,606,180 nodes | 57% |
| An open game after 2...Nc6 | 190,506 | 475,421 | 60% |
| A bishop endgame | 15,237 | 42,065 | 64% |

The table also lasts from one move to the next. When the engine moves and the opponent
replies, the new position was already searched two plies deep in the last search, so the next
search starts with good moves for most positions it visits. `ucinewgame` clears it.

GUIs show how full the table is: `info ... hashfull 312` means 31.2%, estimated from the
first thousand slots.

## Pitfalls

- **Storing garbage.** A search stopped by the clock returns made-up scores. Nothing is
  stored once the search has been stopped.
- **Graph history interaction.** The TT doesn't know how a position was reached. A position
  that is a draw by repetition on one path might be stored as a draw and reused on a path
  where it isn't. Every engine accepts this small inaccuracy.
- **Truncated principal variations.** A TT cutoff ends the line at that position, so the PV
  can come out shorter than the depth. Later stages can rebuild it from the table.
- **Minimax comparisons.** The TT's bounds come from alpha-beta, so the minimax comparison
  test switches it off.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 8 vs stage 7, 8+0.08, SPRT [0, 5] | +48 −9 =230 over 287 games: passed (about +48 ± 17 Elo) |

## Further reading

- Chess Programming Wiki: [Transposition Table](https://www.chessprogramming.org/Transposition_Table),
  [Replacement Strategies](https://www.chessprogramming.org/Replacement_Strategies),
  [Graph History Interaction](https://www.chessprogramming.org/Graph_History_Interaction).
- Albert Zobrist, "A New Hashing Method with Application for Game Playing" (1970): the
  hashing behind the table.
- Daniel Lemire, ["A fast alternative to the modulo reduction"](https://lemire.me/blog/2016/06/27/a-fast-alternative-to-the-modulo-reduction/) (2016).
