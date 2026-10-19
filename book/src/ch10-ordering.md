# Move ordering

*Stage 10 · tag `stage-10-ordering` · code in `crates/engine/src/ordering.rs`*

Alpha-beta's speed depends on searching the best move first. With perfect ordering it visits
about the square root of the positions minimax does; with bad ordering it saves almost nothing.
The stages so far order moves like this:

- the transposition table's move first (chapter 8);
- then captures, by MVV-LVA (chapter 7);
- then quiet moves, in whatever order the generator produced them.

Most moves are quiet, and at many nodes none of the captures is any good, so that last
category matters. This chapter adds three heuristics that learn, from the search itself,
which quiet moves tend to be good. It also changes *how* moves are ordered, so the work isn't
wasted at nodes that cut off early.

## Killer moves

Say White threatens mate with Qh7. At ply 3 the search tries Black's first move, finds that
Qh7# refutes it, and cuts off. Then it tries Black's second move, and Qh7# probably refutes
that too. A quiet move that caused a cutoff at a given ply is a **killer**, and it is tried
early at the same ply in other branches. cb2026 keeps two per ply. When a new killer arrives,
the older one is dropped.

## Countermoves

Many moves have a natural reply: ...a6 is often met by a4, and Bb5 by ...a6. The
**countermove** table remembers, for each opponent move (the piece that moved and the square
it went to), the quiet move that last refuted it, and tries that reply early.

## The history heuristic

Killers and countermoves remember a handful of moves. The **history** table scores every
quiet move, by side, from-square and to-square. When a quiet move causes a cutoff with
`depth` plies left, its score rises by `depth²`, since cutoffs deep in the tree save the most
work. The quiet moves searched before it at that node, which failed to cut off, lose the same
amount. Quiet moves are then searched in order of their history score.

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:update}}
```

(The `previous` argument and the continuation history lines came later, in chapter 10.1.) The
bonus and the update are two small functions, which chapter 10.2's capture history shares:

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:gravity}}
```

The update uses **gravity**: `change − score × |change| / MAX`. A bonus to a score near the
maximum hardly moves it, while a penalty to it moves it a lot. Scores stay within ±16,384, and
the table keeps adapting instead of saturating. Between searches every score is halved, so
old knowledge fades.

## The order, and picking moves lazily

The full order is:

1. the transposition table move;
2. captures and queen promotions, by MVV-LVA;
3. the first killer, the second killer, the countermove;
4. the remaining quiet moves, by history.

Sorting the whole list at every node would waste time: most nodes that cut off do so on the
first or second move, and the order of the rest never matters. So a **move picker** computes
each move's key, then on each request picks the best remaining move. That is one pass of a
selection sort, done only as far as the search actually goes:

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:picker}}
```

The quiescence search takes the same idea further, with **staged generation**. Unless the
side to move is in check, it only wants captures and promotions, so the move generator now
has a mode that generates only those (`Position::noisy_moves`). The pieces' target squares
are restricted to enemy pieces, and pawns only generate captures and promoting pushes. Most
of the positions a search visits are quiescence nodes, so this is where the cheaper
generation pays off. A test checks that this mode generates exactly the captures and
promotions among the legal moves, in every position within two plies of each perft position.

## What it saves

Total nodes to search six positions (openings, middlegames, Kiwipete and an endgame) to
depth 7:

| Setting | Nodes | vs all on |
| --- | ---: | ---: |
| All on | 16,836,067 | — |
| `set killers off` | 16,275,312 | −3% |
| `set countermoves off` | 16,800,724 | 0% |
| `set history off` | 18,126,807 | +8% |
| All three off | 28,854,516 | +71% |

Together they cut the tree by 42%. Each one adds little on top of the others, because they
often agree on the same move: with history and countermoves on, killers even cost a few
percent here. These numbers are from one small set of positions. The real test is games,
below. Later stages (late move reductions, above all) lean on good quiet-move ordering even
more, because they search the late moves less deeply.

## Pitfalls

- **Killers that aren't quiet.** A capture that cuts off is already ordered by MVV-LVA.
  Storing it as a killer would waste the slot, so only quiet moves are recorded.
- **Stale killers.** Killers belong to a ply of one search tree. They are cleared at the start
  of each search, while history is only halved.
- **Unbounded history.** Adding bonuses without gravity or a cap lets a few moves' scores run
  away, and the table stops reflecting recent results.
- **Determinism.** Ties keep generation order, so the search, and the bench count, are
  reproducible.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 10 vs stage 9, 8+0.08, SPRT [0, 5] | +251 −110 =116 over 477 games: passed (about +106 ± 28 Elo) |

## Further reading

- Chess Programming Wiki: [Move Ordering](https://www.chessprogramming.org/Move_Ordering),
  [Killer Heuristic](https://www.chessprogramming.org/Killer_Heuristic),
  [Countermove Heuristic](https://www.chessprogramming.org/Countermove_Heuristic),
  [History Heuristic](https://www.chessprogramming.org/History_Heuristic),
  [Move Generation: staged](https://www.chessprogramming.org/Move_Generation#Staged_Move_Generation).
- Jonathan Schaeffer, "The History Heuristic and Alpha-Beta Search Enhancements in Practice",
  *IEEE Transactions on Pattern Analysis and Machine Intelligence* 11 (1989).
- Jos Uiterwijk, "The Countermove Heuristic", *ICCA Journal* 15 (1992).
