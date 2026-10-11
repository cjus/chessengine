# Quiescence search and MVV-LVA

*Stage 7 · tag `stage-07-quiescence` · code in `crates/engine/src/search.rs` and `ordering.rs`*

Chapter 6 ended with a warning sign. From the starting position the stage 6 engine's score
flipped between +1 and −1 from one depth to the next, as if a pawn appeared and disappeared
with every ply. Nothing of the sort happens in the real game. The search simply stopped in the
middle of exchanges. This chapter fixes that, and the fix is one of the biggest single gains
an engine gets.

## The horizon effect

A fixed-depth search stops at its horizon and evaluates whatever position is there, even if a
piece is hanging. Take this position: White has a queen, Black two pawns.

```text
> fen 4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1
> set depth 1
> set quiescence off
> go
  depth  1    +8.00          18 nodes       0 ms  Qxd5
cb2026 plays Qxd5   (depth 1, +8.00, 18 nodes, 0 ms)
```

At depth 1 the search sees Qxd5, counts material (queen against one pawn, +8), and stops,
without seeing that exd5 wins the queen. Searching deeper doesn't cure this. It only moves the
horizon: at any depth, the last move can be a capture whose recapture lies beyond it.

## Quiescence search

The fix is to keep searching past the horizon, but only moves that change the material
balance: captures and queen promotions. Captures run out quickly, so this ends. The search
goes on until the position is **quiet** (in Latin *quiescens*), and only then is it
evaluated.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:quiescence}}
```

Three details make it work:

- **Stand pat.** The side to move never *has* to capture. Its static evaluation is a lower
  bound on its score, the "I'll just stop here" option. If that's already `≥ beta`, the
  opponent won't allow this position and the search returns at once. Otherwise it raises
  `alpha`, and only captures that do better than standing still matter.
- **Ordering.** The quiescence search is mostly cutoffs, and those depend on trying the
  best capture first (see MVV-LVA below).
- **Checks.** In check, standing pat isn't an option. The position may be lost, so every
  legal move is searched, not just captures. That's how the quiescence search spots a mate
  at the horizon.

Now the same position:

```text
> set quiescence on
> go
  depth  1    +7.00          41 nodes       0 ms  Kf1
cb2026 plays Kf1   (depth 1, +7.00, 41 nodes, 0 ms)
```

The score is right (a queen against two pawns is +7) and the queen stays home. (Kf1 is as
good a quiet move as any to an engine that only counts material. Stage 9 gives it reasons to
prefer one square over another.)

From the starting position the scores stop flipping:

```text
info depth 4 seldepth 7 score cp 0 nodes 3830 time 0 pv b1a3 b8a6 g1f3 a6b4
info depth 5 seldepth 11 score cp 0 nodes 31783 time 5 pv b1a3 b8a6 g1f3 a6b4 a3b1
info depth 6 seldepth 11 score cp 0 nodes 205774 time 42 pv b1a3 b8a6 g1f3 a6b4 a3b1 b4d5
```

`seldepth`, the deepest ply reached, is now larger than `depth`: that's the quiescence search
following captures.

## MVV-LVA

Alpha-beta prunes most when the best move is searched first (chapter 5). Without any
knowledge of the position, the best guess is that captures come first, and among captures:
**Most Valuable Victim, Least Valuable Attacker**. Take the biggest piece you can, and use
your smallest piece to do it. Pawn takes queen comes first, queen takes pawn last.

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:mvv_lva}}
```

In the quiescence search, ordering is not a nicety. In a sharp position like Kiwipete,
searching captures in generation order makes the quiescence search explode: try
`set mvvlva off` and `go depth 3` on it. The ordered search finishes depth 3 in 15,786
nodes and 4 ms. The unordered one, after a second and more than 4 million nodes, has only
got through depth 1. In a quiet opening
position at depth 4 the difference is 12,242 nodes against 178,336.

MVV-LVA has a known blind spot: QxP is ordered before the other captures of a pawn even when
the pawn is defended and the exchange loses the queen. Stage 14 adds static exchange
evaluation (SEE), which works out the whole exchange on a square. Quiet moves keep their
generation order for now; stage 10 orders them with killer moves and the history heuristic.

## Pitfalls

- **Searching all promotions.** Under-promotions almost never matter and multiply the tree,
  so only queen promotions are searched.
- **Standing pat in check.** That would let a mated side "stand pat" at a good score. In
  check, cb2026 searches every evasion and recognises mate.
- **Comparing with minimax.** With alpha-beta off, the quiescence search has no cutoffs at
  all and takes minutes on positions like Kiwipete. The alpha-beta-vs-minimax test in
  `search.rs` switches quiescence off for that reason.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 7 vs stage 6, 8+0.08, SPRT [0, 5] | +123 −31 =128 over 282 games: passed (about +118 ± 30 Elo) |

## Further reading

- Chess Programming Wiki: [Quiescence Search](https://www.chessprogramming.org/Quiescence_Search),
  [Horizon Effect](https://www.chessprogramming.org/Horizon_Effect),
  [MVV-LVA](https://www.chessprogramming.org/MVV-LVA),
  [Move Ordering](https://www.chessprogramming.org/Move_Ordering).
