# ProbCut

*Milestone M5, after stage 20 · on `main`, no stage tag · code in `crates/engine/src/search.rs`*

Null-move pruning (chapter 12) cuts a node off when passing keeps the score at or above beta,
in a search a few plies shallower. **ProbCut** makes a bet of the same shape with a real move:
if some capture beats beta by a clear margin, even in a search four plies shallower, then the
full-depth search would almost surely beat beta too, and the node is cut off.

The bet rests on a fact about search scores: a shallow search's score and a deep search's
score of the same position are closely related (Buro measured it, for Othello). So a shallow
score well above beta says, with high probability, that the deep score is above beta as
well. The name is short for *probabilistic cut*. Like chapter
14.1's techniques, it came after stage 20, so no stage tag contains it. Switch it with
`set probcut on|off`.

## The test

```rust,ignore
{{#include ../../crates/engine/src/search.rs:probcut}}
```

ProbCut asks about `probcut_beta`, beta plus `probcut_margin` (100 centipawns), and it keeps
the cost down three ways:

- **Captures only.** A move that beats beta by a pawn in a shallow search is usually one that
  wins material. Quiet moves are left to the full search.
- **Only captures that could make it.** A capture's static exchange evaluation (chapter 14)
  has to make up what the static evaluation is short of `probcut_beta`. In a position
  evaluated at beta, that is a capture winning a pawn or more. Ten points below, it has to
  win 110.
- **The quiescence search first.** Each capture gets a quiescence search against
  `probcut_beta`, which costs little. Only if that reaches the margin does the capture get
  the real test, a null-window search `probcut_reduction` (4) plies shallower than the node.

A capture that passes the test cuts the node off. The cutoff is stored in the transposition
table as a lower bound, at the depth of ProbCut's search plus one: one ply more because the
capture was made before that search.

## The conditions

- **Off the principal variation and out of check**, like the pruning of chapters 12 to 14.
- **From depth 5** (`probcut_min_depth`). At depth 4, ProbCut's own search would be only the
  quiescence search.
- **Not near a mate score.** A margin added to a mate score is no longer a score.
- **Not when the hash table already knows better.** If the table holds an exact score or an
  upper bound for this position, from a search at least as deep as ProbCut's own, and that
  score is below `probcut_beta`, ProbCut's search would probably fail too, so it isn't tried.
  A lower bound below `probcut_beta` doesn't say the true score is below it, so it doesn't
  stop the test.
- **Not in a singular extension's search**, which leaves the hash move out and asks a
  different question.

## Where it comes from

Michael Buro invented ProbCut for Othello, in his thesis (1994) and in "ProbCut: An Effective
Selective Extension of the Alpha-Beta Algorithm" (ICCA Journal, 1995). His version is
statistical. It measures, over many positions, how a deep search's score relates to a
shallow one's, and sets the margin from the spread, so that a cut is wrong with a chosen,
small probability. It worked very well in Othello. In chess, attempts in Crafty in 2003 and
2004 found it no better, or worse.

The version that works in chess is Gary Linscott's for Stockfish (GPL-3.0, 2013): a fixed
margin, captures only, and a search a few plies shallower. cb2026's follows that form and adds
the quiescence search first and the hash table check, as later Stockfish versions do. The
code is cb2026's own. The three numbers, 5, 100 and 4, were set by hand, not tuned.

## What it changes

The bench is 9.6% smaller with it: 963,637 nodes instead of 1,065,754. With
`set probcut off` it is `main`'s exactly. Three alternating bench runs gave 1.55 to 1.62
million nodes a second for `main` and 1.63 to 1.65 million with ProbCut.

The `stats` command shows how often it fires. After a search to depth 12:

| Position | ProbCut tried | Cut off | Nodes with it ÷ without (`ab probcut 12`) |
| --- | ---: | ---: | ---: |
| Kiwipete | 3,677 nodes | 2,512 (68%) | 1.00 |
| An Italian Game middlegame | 947 | 334 (35%) | 1.01 |
| A queen's pawn middlegame | 1,209 | 424 (35%) | 1.02 |

On those three, ProbCut cuts off hundreds or thousands of nodes, yet the tree comes out about
the same size. Presumably its own searches cost about what its cutoffs save there; on the bench
positions it saves 9.6%. Whether it is worth Elo, only games can say.

## Pitfalls

- **The margin is a bet.** Too small, and ProbCut cuts nodes whose deep score would have
  fallen below beta. Too large, and it seldom fires and only costs time. The right size
  depends on the evaluation: a pawn here, on cb2026's centipawn scale.
- **Cost at nodes that don't cut off.** Each capture tried costs a quiescence search, and
  some a shallower search too. Where no capture reaches the margin, that work is wasted.
  The static exchange filter and the quiescence search first keep it small.
- **Mate scores.** A margin added to a score near mate makes no sense, so ProbCut isn't tried
  there. A mate it finds through the capture is a real one, so it is returned as it is.
- **The hash table.** A ProbCut result is a lower bound from a shallower search, stored at
  that depth, so a later search that needs more depth searches the node again.

## What it's worth

| Test | Result |
| --- | --- |
| ProbCut vs `main` without it, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | Waiting for its SPRT (cjus/chessengine#117) |

## Further reading

- Chess Programming Wiki: [ProbCut](https://www.chessprogramming.org/ProbCut), with Multi-ProbCut
  and the Crafty experiments.
- Michael Buro, "ProbCut: An Effective Selective Extension of the Alpha-Beta Algorithm", ICCA
  Journal 18(2), 1995.
- Stockfish's source (GPL-3.0), `search.cpp`, "ProbCut".
