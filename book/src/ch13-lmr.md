# Late move reductions and pruning

*Stage 13 · tag `stage-13-lmr` · code in `crates/engine/src/search.rs`*

Good move ordering (chapter 10) means a cutoff almost always comes from one of the first few
moves searched. So most of the effort at a node goes into proving that the *other* moves are
no better, and those moves are usually poor. This chapter spends less on them. It is the
single biggest step in depth an engine takes.

## Late move reductions

**Late move reductions** (LMR) search the late moves at a reduced depth. The later a move
comes in the ordering and the deeper the search, the bigger the reduction:

```text
reduction = 0.75 + ln(depth) × ln(move number) / 2.25      (rounded down)
```

At depth 10, the 20th move is searched 3 plies less deep. At depth 3, the third move loses 1
ply. The table is computed once:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:lmr_table}}
```

A reduced search is a guess, so it has a safety net. If the reduced, null-window search says
the move beats alpha after all, the move is searched again at full depth. If it still beats
alpha and lies within the window, PVS searches it once more with the full window:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:pvs}}
```

Some moves are never reduced, because they are too often important:

- the first move (usually the TT move);
- captures and promotions;
- moves that give check, and every move when in check;
- moves at depths below 3, or among the first 3 moves.

On the principal variation, where the exact score matters, reductions are one ply smaller.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:lmr}}
```

(The history adjustment came later, in chapter 14.1.)

## Late move pruning

Near the leaves, LMR's reductions would leave little depth to search. **Late move pruning**
(LMP) goes one step further. At depth 3 or less, outside the principal variation, once
`3 + depth²` quiet moves have been tried, the remaining quiet moves are skipped entirely: 4 at
depth 1, 7 at depth 2, 12 at depth 3.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:lmp}}
```

Pruning only starts once some move has been found that doesn't lose to a mate. Otherwise a
side that is under a mating attack could prune the one quiet move that defends.

## What it saves

Six positions to depth 8, as in chapters 10 to 12:

| Setting | Nodes | vs both on |
| --- | ---: | ---: |
| Both on | 1,389,854 | — |
| `set lmr off` | 3,814,336 | 2.7× |
| `set lmp off` | 2,488,721 | 1.8× |
| Both off | 9,996,424 | 7.2× |

The tree is seven times smaller. From the starting position cb2026 now reaches depth 16 in
about two seconds. At stage 9 the same time bought depth 9. The depth numbers are no
longer comparable with the early stages: a "depth 16" search with LMR has many lines searched
far less deeply than 16 plies.

## Pitfalls

- **Reductions that are never verified.** Without the full-depth re-search, LMR would
  silently miss every late move that turns out to be good. The re-search is what makes it
  safe enough.
- **Reducing the wrong moves.** Captures, checks and evasions often change the evaluation
  sharply. Reducing them hides tactics.
- **Ordering matters more than ever.** A good move ordered late now gets a shallower search.
  Every improvement to ordering (chapter 10, and better history in later stages) pays off
  twice.
- **Tuning.** The 0.75 and 2.25 in the formula, the 3 move and depth thresholds and LMP's
  `3 + depth²` are reasonable starting values, not measured optima. Each is a named
  parameter in `SearchParams`, ready for tuning.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 13 vs stage 12, 8+0.08, SPRT [0, 5] | +73 −5 =35 over 113 games: passed (about +242 ± 59 Elo) |

## Further reading

- Chess Programming Wiki: [Late Move Reductions](https://www.chessprogramming.org/Late_Move_Reductions),
  [Futility Pruning: Move Count Based Pruning](https://www.chessprogramming.org/Futility_Pruning#MoveCountBasedPruning),
  [Reductions](https://www.chessprogramming.org/Reductions).
- Tord Romstad, "An Introduction to Late Move Reductions" (2006): the article that made LMR
  widely known.
