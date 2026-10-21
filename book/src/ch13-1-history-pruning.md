# History pruning

*Milestone M5, after stage 20 · on `main`, no stage tag · code in `crates/engine/src/search.rs`*

Late move pruning (chapter 13) skips quiet moves near the leaves by how late they come: at
depth 3, after twelve quiet moves, the rest are skipped. It doesn't look at the moves
themselves. **History pruning** does: near the leaves, it skips a quiet move whose history
says it keeps failing where other moves cut off. The worse the history, the deeper it may
prune.

The history tables (chapters 10 and 10.1) already order the quiet moves, and history in late
move reductions (chapter 14.1) reduces a move with a bad history more. History pruning takes
the last step and doesn't search such a move at all. Like chapter 14.1's techniques, it came
after stage 20, so no stage tag contains it. It measured weaker, so it is **off by default**;
`set histprune on` switches it on.

## The test

```rust,ignore
{{#include ../../crates/engine/src/search.rs:history_pruning}}
```

A move is skipped when its quiet-move score, history plus continuation history (the score
that orders the quiet moves), is below `-history_pruning_margin` (16) per ply of depth, up to
`history_pruning_max_depth` (4). A cutoff at depth 4 gives the quiet moves tried before it a
penalty of 16 each, so at depth `d` the move is pruned when its score is worse than `d` such
penalties. Like late move pruning and futility pruning, it never applies:

- on the principal variation, whose exact score matters;
- in check, or to a move that gives check;
- before a move has been searched: the first move never goes, and if every move searched so
  far is mated, nothing is pruned, so a mate can't be missed by skipping the only defence.

## Which score

Stockfish (GPL-3.0) prunes by continuation history alone. In cb2026 that score turned out to
be a poor sign. The test was over the positions of one game: each searched to depth 10, one
after another in the same shell, so that the tables fill as they do in a game. At the nodes
where history pruning could apply, each quiet move searched was counted by its score and by
whether it raised alpha:

| Depth | All | Quiet-move score below −16 × depth | Quiet-move score 0 | Quiet-move score above 0 | Continuation history below 0 | Continuation history 0 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 6.1% | 6.0% | 1.0% | 7.0% | 3.7% | 1.6% |
| 2 | 4.6% | 2.9% | 1.3% | 6.6% | 2.4% | 1.2% |
| 3 | 2.2% | 0.5% | 0.7% | 5.0% | 1.3% | 0.4% |
| 4 | 0.7% | 0.3% | 0.3% | 3.0% | 0.4% | 0.1% |

A move with a negative continuation score raised alpha two to three times as often as one with
no score at all, at every depth. A score of zero mostly means the move was never tried in this
context. A negative one means it was tried at a node where another quiet move then cut off:
only those nodes hand out penalties. So negative scores pile up at nodes that cut off, where
any move is more likely to fail high, and the sign says as much about the node as about the
move.

The quiet-move score, which adds the history table, does better at depths 3 and 4. There, a
move below the margin raised alpha about as rarely as a move with no score, against 3% to 5%
for a move with a positive score. At depths 1 and 2 it still raised alpha often. But those are
late in the order there, and late move pruning skips most of them anyway. The margin and the
depth limit were set from these measurements, not tuned.

## Where it comes from

The Chess Programming Wiki traces the idea to an option in a 2005 version of Fruit, by Fabien
Letouzey: *history leaf pruning*, which skipped moves near the leaves below a history
threshold. Stockfish (GPL-3.0) uses the form here: a threshold that grows with the depth, on
the continuation history of the moves one, two and four plies back. cb2026 keeps only two
of those and adds the history table. The code is cb2026's own.

## What it changes

With it on, the bench is 10.0% smaller: 1,022,919 nodes instead of 1,136,500. Off, it is
`main`'s exactly. Three alternating pairs of bench runs gave speed
ratios of 0.98 to 1.00: the score is a few table reads, and the history tables are in the cache.

The six positions of chapters 10 to 14, each searched from empty tables:

| Depth | `set histprune off` | History pruning | Moves it skipped |
| --- | ---: | ---: | ---: |
| 9 | 447,273 | 425,452 (−5%) | 16,720 |
| 10 | 703,714 | 580,284 (−18%) | 38,355 |
| 11 | 885,651 | 830,827 (−6%) | 90,711 |

Over the game's 58 positions at depth 10, with the tables carried over, it skipped 261,901
moves, against 1,494,283 skipped by late move pruning and 378,100 by futility pruning. Even so,
the trees came out about the same size: 5% more nodes in all, smaller in 30 of the 58
positions. Other limits did no better: up to depth 6, or with half the margin, the average
change per position (a geometric mean) went from 10% smaller to 22% bigger. Pruning moves
reshuffles the tree as much as it shrinks it, and tree sizes have not predicted the games here
(chapters 10.2, 12.1 and 14.1).

## Pitfalls

- **The scores are small.** A cutoff's bonus is `depth²`, so most scores stay within a few
  hundred of zero, far from the limit of 16,384. A margin copied from an engine with other
  bonuses would prune nothing, or everything. Stockfish's bonus is up to 1,561 a cutoff, and
  its threshold is 3,832 a ply.
- **A bad score isn't always a bad move.** The table in "Which score" shows a negative
  continuation score marking moves that matter more often than unscored ones.
- **The tables have to fill.** A short search from empty tables has few scores that low: the
  unit test, eight plies with a material-only evaluation, needs a margin of 1 to see any
  pruning. The six positions at depth 9 already prune thousands of moves.
- **Node counts at one depth swing.** With history pruning on, IIR's unit test found IIR's
  tree 8% bigger on Kiwipete at depth 8, though IIR saves nodes at depths 7, 9 and 10 and over
  the six positions. The test now searches to depth 9.

## What it's worth

| Test | Result |
| --- | --- |
| History pruning vs `main` without it, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **−5.3 ± 7.1 Elo** over 4,548 games (+1087 −1156 =2305): failed (LLR −2.97), cjus/chessengine#140, so it is off by default |

Stockfish's own estimate for its version is about 2 Elo, in an engine where many techniques
already overlap, so a small result was expected; this one leans the other way. Like capture
history, ProbCut and others (chapters 10.2, 12.1 and 14.1), it made the bench smaller and the
engine no stronger. The switch and both parameters stay. Untested candidates for another try:

- **From depth 3 only.** At depth 1 the moves it skips raised alpha as often as quiet moves in
  general, and at depth 2 more than half as often ("Which score"), so pruning them there may
  cost the most. A
  `history_pruning_min_depth` would be a new parameter.
- **A larger margin**, 32 a ply: a quarter as many moves skipped over the game's positions.
- **The long time control**, 40+0.4, where the history tables are fuller and the searches deeper.

## Further reading

- Chess Programming Wiki:
  [History Leaf Pruning](https://www.chessprogramming.org/History_Leaf_Pruning),
  [History Heuristic](https://www.chessprogramming.org/History_Heuristic).
- Stockfish's source (GPL-3.0), `search.cpp`, "Continuation history based pruning".
