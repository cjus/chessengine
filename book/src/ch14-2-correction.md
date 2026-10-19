# Correction history

*Milestone M5, after stage 20 · on `main`, no stage tag · code in
`crates/engine/src/correction.rs` and `crates/engine/src/search.rs`*

Chapters 12 to 14 prune by guessing from the static evaluation. Reverse futility pruning returns
when the evaluation is far above beta, razoring gives up when it is far below alpha, null-move
pruning needs it at beta or above, and futility pruning skips quiet moves that can't lift it to
alpha. The quiescence search stands pat on it at every leaf. All of them are only as good as
that one number.

Chapter 15's evaluation is hand-crafted, and it is wrong in patterns. In some pawn structures it
is too optimistic, in others too pessimistic, by much the same amount each time the structure
comes up: a weakness it doesn't know about, or a pawn chain it overrates. The search finds out
as it goes. A node's searched score is a better estimate than its static evaluation, and the
difference between them is the evaluation's error in that position.

**Correction history** remembers those errors. It keeps a running average of the difference
in a table indexed by the side to move and the pawn structure, and adds the average to every
later static evaluation of a position with the same pawns. Like chapter 14.1's techniques, it
came after stage 20, so no stage tag contains it. Switch it with `set correction on|off`.

## The table

A pawn structure is two bitboards, White's pawns and Black's. There are far too many structures
to give each its own entry, so the two bitboards are mixed into one number, and its low bits pick
one of 16,384 entries per side:

```rust,ignore
{{#include ../../crates/engine/src/correction.rs:pawn_key}}
```

Black's pawns are mixed before they are combined with White's, so that a white pawn on a2 and a
black pawn on a2 don't count as the same structure. The mixer is SplitMix64's finaliser, a few
shifts and multiplications that make every bit of the input affect every bit of the output, so
similar structures land far apart. Engines usually keep a pawn hash key up to date move by move,
as the position's Zobrist key is (chapter 4). Computing it from the bitboards instead costs much
less than the evaluation it corrects and leaves the position as it is.

The entries are kept in 256ths of a centipawn:

```rust,ignore
{{#include ../../crates/engine/src/correction.rs:table}}
```

## Correcting

Every node in the main search computes its static evaluation, then corrects it:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:static_eval}}
```

```rust,ignore
{{#include ../../crates/engine/src/correction.rs:corrected}}
```

The quiescence search corrects its stand pat the same way. The uncorrected evaluation is kept
too, because that is what the table learns about.

## Learning

When a node's search is done, its result says something about the evaluation, but not always:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:correction_update}}
```

```rust,ignore
{{#include ../../crates/engine/src/correction.rs:update}}
```

- **Captures and promotions.** If the best move wins material, the score is higher than the
  evaluation because of the capture. The evaluation of a position isn't meant to include
  material that is about to change hands; the quiescence search is there for that.
- **Bounds.** A fail high is a lower bound: the true score is at least `score`. If that is above
  the evaluation, the evaluation was too low by at least the difference; if it is below, the
  true score could be anything, and the node says nothing. A fail low is the mirror image.
- **Mate scores** are about a forced line, not the position's evaluation.
- **No static evaluation in check**, and a singular extension's search leaves the best move out
  on purpose, so neither counts.

An entry then moves towards the new difference by a fraction of the way:
`min(depth + 1, 16)` 256ths. A deep search's result is worth more than a shallow one's, but even
the deepest moves the entry only a sixteenth of the way, so the entry is an average over many
nodes and no single surprising result dominates. No entry goes past 64 centipawns either way.
Both numbers are search parameters, `correction_max_weight` and `correction_limit`.

## What it changes

The bench, which starts every position with an empty table, is 3.0% smaller with it: 1,065,754
nodes instead of 1,098,926. The speed is unchanged: computing the pawn key at every node costs
nothing measurable.

The corrections themselves are small. After searching the 14 bench positions to depth 13 one
after another, without clearing the table, 2,111 of the 32,768 entries were in use. Their
average correction was 3.5 centipawns, 90% were within 8 and 99% within 25, and none reached
the limit of 64. Reverse futility pruning, razoring and futility pruning have margins of 80
centipawns a ply or more, so a typical correction changes few of their decisions. Null-move
pruning, which only needs the evaluation at beta, and the stand pat feel it more directly.

Even so, it gained about as much as singular extensions and multi-cut (+29.5, chapter 14.1),
among the largest of the changes made after stage 20. The numbers above don't explain that on
their own, and two things they leave out probably do. In a game the table carries over from
move to move and the same structures come up again and again, so its entries learn from far
more nodes than in that measurement. And most of the gain may come from the few structures
where the evaluation is consistently wrong by more, the 1% beyond 25 centipawns, which are also
where a wrong evaluation costs the most. Neither is measured.

## Pitfalls

- **Learning from the correction itself.** The searched score comes from a tree whose leaves
  were corrected. If the table learnt the difference from the *corrected* evaluation, each
  correction would build on the last. Measured against the raw evaluation, an entry is an average
  of how far the raw evaluation was off, and it settles.
- **Collisions.** Every pawn structure the search meets shares 16,384 entries per side, so
  unrelated structures can share an average. A bigger table collides less, but each entry learns
  more slowly.
- **The mate range.** A corrected evaluation is clamped short of the mate scores, so that a
  correction can never make a large evaluation look like a forced mate.
- **Helper threads** (chapter 17) start every search with an empty table of their own; only the
  main thread's table carries over from one move to the next. What they find still reaches the
  main thread through the shared transposition table.
- **Comparisons at equal scores.** Correction history changes the static evaluation, so it
  changes scores, not only node counts. `Techniques::sound()`, which keeps only the techniques
  that don't change the result, switches it off.

## What it's worth

| Test | Result |
| --- | --- |
| Correction history vs `main` without it, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+28.0 ± 13.0 Elo** over 1,308 games (+362 −257 =689): passed (LLR 2.95), cjus/chessengine#112 |

## Further reading

- Chess Programming Wiki:
  [Static Evaluation Correction History](https://www.chessprogramming.org/Static_Evaluation_Correction_History).
- The idea comes from [Caissa](https://github.com/Witek902/Caissa) (MIT licence, 2023), and
  Stockfish (GPL-3.0) and most strong engines have adopted it, several with more tables than
  the pawn one. The code here is cb2026's own.
- Guy L. Steele, Doug Lea and Christine H. Flood, "Fast Splittable Pseudorandom Number
  Generators", OOPSLA 2014: SplitMix64, whose finaliser mixes the pawn key.
