# A full hand-crafted evaluation

*Stage 15 · tag `stage-15-hce` · code in `crates/engine/src/hce.rs` and `eval.rs`*

The search is now deep and selective. What it finds at the leaves is still judged by
chapter 9's evaluation: material and piece-square tables. Those know that a knight likes the
centre, but nothing about how the pieces relate to each other. A bishop on its best square
counts the same whether its diagonals are open or blocked by its own pawns. A king behind
three pawns counts the same as one whose shelter has been torn away. This chapter adds the
classic positional terms of a hand-crafted evaluation (HCE).

## The terms

```rust,ignore
{{#include ../../crates/engine/src/hce.rs:params}}
```

### Mobility

A piece is worth more when it has more places to go. For each knight, bishop, rook and queen,
cb2026 counts the squares it attacks that are neither occupied by its own side nor covered by
an enemy pawn: squares it could actually use. Each piece type scores per square above or
below a typical count. A bishop with 11 safe squares beats one with 3, and that's how the
evaluation learns about "good" and "bad" bishops without a rule for each.

### Pawn structure

- **Doubled pawns**, two on one file, can't protect each other, and the rear one is often
  blocked.
- **Isolated pawns**, with no friendly pawn on either neighbouring file, can only be defended
  by pieces.
- **Passed pawns** have no enemy pawn ahead on their own or a neighbouring file, so nothing
  but pieces can stop them. Their bonus climbs steeply with each rank, and more steeply in
  the endgame, where few pieces remain to stop them.

### Pieces

- **The bishop pair**: two bishops cover both square colours and work well together in open
  positions, worth about a third of a pawn more in the middlegame and half a pawn in the
  endgame.
- **Rooks on open files** (no pawns) or **semi-open files** (no pawns of their own) can
  reach the enemy position.

### King safety

In the middlegame the king needs shelter, and the danger it faces depends on how many enemy
pieces are bearing down on it.

- The **pawn shield** scores each friendly pawn in the 3×2 block in front of the king.
- **King attacks** count, for each enemy piece, how many squares of the **king zone** (the
  king's square and its neighbours) it attacks, weighted by piece type: knight and bishop 2,
  rook 3, queen 5. The penalty is the *square* of the total, scaled and capped. One piece
  alone rarely mates, but several together often do, and the quadratic shape captures that.

King safety only has middlegame values. As pieces come off, the phase blend fades it out by
itself.

### Tempo

The side to move gets a small bonus: having the move is worth something. The evaluation is
still symmetric, since mirroring a position (swapping colours and flipping the board) also
swaps the side to move.

```rust,ignore
{{#include ../../crates/engine/src/hce.rs:terms}}
```

## The trace

Every term is recorded for each side separately, so the shell's `eval` command can show
exactly where a score comes from. Here is the Italian Game again, with White's king walked to
e2:

```text
> fen r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPPKPPP/R1BQ3R w - - 0 1
> eval
Evaluation in pawns, each side from its own point of view:
                             White           Black      Net
                    Middle     End  Middle     End    (W-B)
  Material          +39.40  +42.00  +39.40  +42.00    +0.00
  Piece-squares      +0.20   -0.15   +0.75   -0.50    -0.55
  Mobility           -0.61   -0.91   -0.61   -0.96    +0.00
  Pawn structure     +0.00   +0.00   +0.00   +0.00    +0.00
  Passed pawns       +0.00   +0.00   +0.00   +0.00    +0.00
  Pieces             +0.30   +0.50   +0.30   +0.50    +0.00
  King safety        +0.10   +0.00   +0.44   +0.00    -0.34
  Tempo              +0.10   +0.05   +0.00   +0.00    +0.10
Phase 24/24: 100% middlegame, 0% endgame
Total: -0.79 for White, so -0.79 for White to move
```

Black's castled king has three shield pawns. White's king has one shield pawn (e4) and is
worse placed on the piece-square table. That costs White most of a pawn, which is about what
a strong player would say. Mobility is negative for both sides only because it is measured
against the typical counts of a more open position. Only the difference between the sides
matters.

`set mobility off`, `set pawns off`, `set pieces off`, `set kingsafety off` and `set tempo off`
switch the groups off one at a time.

## Getting it right

An evaluation bug doesn't crash anything; it just makes the engine play worse. Two kinds of
test catch most of them:

- **Symmetry.** Every bench position and its mirror image must get the same score for the
  side to move. A term that forgets to flip a rank for Black fails this at once.
- **Hand-checked positions.** Small positions where each term's value can be worked out by
  hand: doubled and isolated pawns, a passed pawn on the seventh, the bishop pair with a rook
  on an open file, a sheltered king against an exposed one.

## Pitfalls

- **Double counting.** The piece-square tables already reward advanced pawns and central
  knights. The passed-pawn and mobility bonuses add to that, so the values have to be set
  with both in mind. Stage 16's tuning sets them together.
- **Speed.** Mobility and king attacks generate attacks for every piece at every leaf. On
  the bench, the engine searches about a quarter fewer nodes per second than with material
  and tables alone. The extra knowledge has to win more than the depth it costs. The SPRT
  is the judge.
- **Hand-set numbers.** Every value here is a reasonable guess. The next chapter replaces
  guessing with fitting them to game results.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 15 vs stage 14, 8+0.08, SPRT [0, 5] | +163 −41 =39 over 243 games: passed (about +192 ± 45 Elo) |

## Further reading

- Chess Programming Wiki: [Evaluation](https://www.chessprogramming.org/Evaluation),
  [Mobility](https://www.chessprogramming.org/Mobility),
  [Pawn Structure](https://www.chessprogramming.org/Pawn_Structure),
  [Passed Pawn](https://www.chessprogramming.org/Passed_Pawn),
  [Bishop Pair](https://www.chessprogramming.org/Bishop_Pair),
  [Rook on Open File](https://www.chessprogramming.org/Rook_on_Open_File),
  [King Safety](https://www.chessprogramming.org/King_Safety).
