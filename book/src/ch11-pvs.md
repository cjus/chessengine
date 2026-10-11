# Principal variation search and aspiration windows

*Stage 11 · tag `stage-11-pvs` · code in `crates/engine/src/search.rs`*

Chapters 7 to 10 made the first move searched at a node very likely to be the best one. This
chapter puts that to work. Both ideas here change the **window** `[alpha, beta]` a search is
given, and both rest on the same fact: the narrower the window, the more alpha-beta can
prune.

## Null-window searches

What does a search with the window `[alpha, alpha + 1]` do? No integer score fits strictly
between alpha and alpha + 1, so it can't compute an exact score. It can only answer a yes/no
question, *is this position worth more than alpha?*, and it answers with a bound. That is
enough to prune almost everything: every move that is clearly worse fails low at once, and
the first move that is better fails high and ends the search.

## Principal variation search

At a node where move ordering has done its job, the first move is the best and every other
move only needs to be *proven worse* than it, not given an exact score. **Principal
variation search** (PVS) does exactly that:

1. Search the first move with the full window `[alpha, beta]`.
2. Search every later move with the null window `[alpha, alpha + 1]`.
3. If a later move unexpectedly scores above alpha, the bet failed: search that move again
   with the full window to get its real score.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:pvs}}
```

The re-searches cost something, but with good ordering they are rare, and the null-window
searches are much cheaper than full ones. The name comes from the **principal variation**:
the line both sides are expected to play, which is the only one that gets full-window
searches.

## Aspiration windows

At the root, iterative deepening gives a good guess before each depth starts: the previous
depth's score. Scores usually move by a few centipawns from one depth to the next. So
instead of the infinite window, each depth from 5 on starts with **aspiration window**
`[previous − 25, previous + 25]`.

- If the score lands inside, the search was cheaper than with the full window, and the
  answer is exact.
- If it **fails low** (the position is worse than thought) or **fails high** (better), the
  window is widened on that side, the margin doubled, and the depth searched again. After a
  fail high, the move that failed high goes first in the re-search. Past a margin of 800 the
  search gives up on guessing and uses the full window.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:aspiration}}
```

```rust,ignore
{{#include ../../crates/engine/src/search.rs:search_params}}
```

## What it saves

Total nodes over six positions at depth 8 (the same positions as chapter 10):

| Setting | Nodes | vs both on |
| --- | ---: | ---: |
| Both on | 40,634,830 | — |
| `set pvs off` | 46,234,900 | +14% |
| `set aspiration off` | 40,783,218 | 0% |
| Both off | 56,198,161 | +38% |

PVS does most of the work. Aspiration windows hardly change the totals on their own, but
they and PVS help each other: the narrower root window makes PVS's re-searches rarer.
Aspiration windows matter more in long searches, where every saved node at the root
multiplies down a deep tree.

## Pitfalls

- **Search instability.** With a transposition table, a re-search doesn't always reproduce
  the result that triggered it: a move fails high on the null window, then scores below
  alpha on the full one. The search must cope (cb2026 simply keeps the result of the
  re-search), not loop forever.
- **Mate scores and aspiration.** A window around a mate score makes no sense, since mate
  scores jump by large amounts between depths. Aspiration is skipped when the previous score
  was a mate.
- **Fail-soft bounds.** When a root search fails, the returned score (beyond the window) is
  used to place the next window, which wastes fewer re-searches than widening blindly.
- **Stopping mid-re-search.** If time runs out while re-searching after a fail high, the
  move that failed high is still the best information there is, so it is kept.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 11 vs stage 10, 8+0.08, SPRT [0, 5], cloud | +340 −297 =213 over 850 games, about +18 ± 20 Elo: no verdict (LLR 0.71) when the run hit the session's time limit |
| Stage 11 vs stage 10, 8+0.08, SPRT [0, 5], test Mac | Pending |

PVS and aspiration windows are worth much less than the early stages, so proving the gain takes
thousands of games. That makes this the first stage whose test runs on the faster test Mac.

## Further reading

- Chess Programming Wiki: [Principal Variation Search](https://www.chessprogramming.org/Principal_Variation_Search),
  [Null Window](https://www.chessprogramming.org/Null_Window),
  [Aspiration Windows](https://www.chessprogramming.org/Aspiration_Windows),
  [Search Instability](https://www.chessprogramming.org/Search_Instability).
- Tony Marsland and Murray Campbell, "Parallel Search of Strongly Ordered Game Trees",
  *ACM Computing Surveys* 14 (1982): the principal variation idea.
