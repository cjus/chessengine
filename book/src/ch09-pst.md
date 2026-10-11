# Piece-square tables and tapered evaluation

*Stage 9 · tag `stage-09-pst` · code in `crates/engine/src/eval.rs`*

Until now cb2026 counted material and nothing else. All quiet moves looked equally good to
it, so it played whichever one the move generator listed first: in chapter 7 it kept its
queen safe with Kf1, and from the starting position it played Na3. This chapter teaches
it where its pieces belong.

## Piece-square tables

A **piece-square table** (PST) gives a piece type a bonus or penalty on each of the 64
squares. The idea is old and cheap: the evaluation adds one table lookup per piece. What goes
into the tables is basic chess knowledge:

- **Knights** belong in the centre. On e5 a knight reaches eight squares, on a1 only two
  ("a knight on the rim is dim").
- **Bishops** like long, open diagonals and dislike corners.
- **Rooks** like the seventh rank, where they attack pawns and hem in the king.
- **Pawns** gain value as they advance, the centre pawns most. Unmoved d- and e-pawns are
  penalised, since they block the bishops.
- **The king** hides behind its pawns in the corner while there are pieces around to attack
  it.

cb2026's tables are written like a printed board, from White's side with rank 8 at the top,
so they can be read at a glance. Here is the knight's, in centipawns:

```text
-50 -35 -25 -20 -20 -25 -35 -50      8
-35 -15   0   5   5   0 -15 -35      7
-25   5  15  20  20  15   5 -25      6
-20   5  20  25  25  20   5 -20      5
-20   0  15  25  25  15   0 -20      4
-25   5  10  15  15  10   5 -25      3
-35 -15   0   5   5   0 -15 -35      2
-50 -30 -25 -20 -20 -25 -30 -50      1
 a   b   c   d   e   f   g   h
```

A black piece uses the same table, flipped: Black's back rank is its rank 1. The flip is one
XOR on the square number:

```rust,ignore
{{#include ../../crates/engine/src/eval.rs:pst_index}}
```

The numbers are cb2026's own, set by hand from the principles above. They don't need to be
exact. Stage 16 tunes all of them automatically on hundreds of thousands of positions from
self-play games.

## Tapered evaluation

The king's table shows the problem with a single set of values. In the middlegame the king
must hide, but in an endgame it is a strong piece that should march to the centre. Pawns,
too, matter much more as the board empties and promotion gets closer.

So every value comes in two versions, **middlegame** and **endgame**, and the evaluation
blends them by the **game phase**: a count of the pieces left on the board.

```text
phase = knights + bishops + 2 × rooks + 4 × queens        24 at the start
score = (mg × phase + eg × (24 − phase)) / 24
```

At the start the score is all middlegame. With only kings and pawns left it is all endgame.
After one rook pair is traded it is 20/24 middlegame. The blend changes a little with each
trade, so the score never jumps, and the search has no reason to seek or avoid a trade just
to cross a threshold.

```rust,ignore
{{#include ../../crates/engine/src/eval.rs:params}}
```

```rust,ignore
{{#include ../../crates/engine/src/eval.rs:evaluate}}
```

The piece values are tapered too: pawns and rooks are worth more in the endgame.

## In the shell

`eval` shows the evaluation term by term. Here's the Italian Game with White's king walked
to e2 instead of castled:

```text
> fen r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPPKPPP/R1BQ3R w - - 0 1
> eval
Evaluation, from White's side, in pawns:
                      Middle       End
  Material             +0.00     +0.00
  Piece-squares        -0.55     +0.35
  Sum                  -0.55     +0.35
Phase 24/24: 100% middlegame, 0% endgame
Total: -0.55 for White, so -0.55 for White to move
```

The king on e2 costs White about half a pawn in the middlegame and would be a plus in an
endgame. With all the pieces on the board, the middlegame figure counts.

`set pst off` and `set tapered off` switch the two ideas off separately, to measure each.

From the starting position cb2026 now develops its knights instead of playing Na3:

```text
info depth 7 seldepth 18 score cp 25 nodes 644665 nps 6024906 time 107 hashfull 47 pv b1c3 b8c6 g1f3 g8f6 d2d4 d7d5 c1e3
```

## Pitfalls

- **Asymmetry bugs.** A mistake in flipping tables for Black makes the evaluation favour one
  colour. The test `mirrored_positions_score_the_same_for_the_side_to_move` evaluates every
  bench position and its colour-flipped mirror and requires the same score.
- **Too much positional weight.** Big table values let the engine give away material for
  "good squares". Tables stay small next to piece values until tuning says otherwise.
- **Phase jumps.** Switching tables at a fixed amount of material, instead of blending,
  makes the evaluation jump when one piece is traded, and the search plays for that jump.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 9 vs stage 8, 8+0.08, SPRT [0, 5] | +14 −0 =0 over 14 games: passed |

The test stopped after 14 straight wins, too few games for a meaningful Elo figure. A material-only engine that doesn't know where its pieces belong has no chance against one that does.

## Further reading

- Chess Programming Wiki: [Piece-Square Tables](https://www.chessprogramming.org/Piece-Square_Tables),
  [Tapered Eval](https://www.chessprogramming.org/Tapered_Eval),
  [Game Phase](https://www.chessprogramming.org/Game_Phase),
  [Simplified Evaluation Function](https://www.chessprogramming.org/Simplified_Evaluation_Function)
  (a well-known hand-written set of tables, by Tomasz Michniewski).
