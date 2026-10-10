# Material, negamax and alpha-beta

*Stage 5 · tag `stage-05-alphabeta` · code in `crates/engine`*

This is where cb2026 starts to play chess. It looks a few moves ahead, counts material at the
end of each line, and picks the move whose worst case is best. That's enough to win material,
avoid losing it, and find mates. Against the stage 4 random mover it won 285 of 286 games.

## Evaluation: counting material

The search can't see to the end of the game, so at the edge of its tree it needs a number
that says how good a position looks. The simplest useful one counts material in centipawns
(hundredths of a pawn), from the point of view of the side to move:

```rust,ignore
{{#include ../../crates/engine/src/eval.rs:params}}
```

```rust,ignore
{{#include ../../crates/engine/src/eval.rs:evaluate}}
```

The values are named parameters, not constants buried in code. Stage 16 tunes all the
evaluation's numbers automatically, and that only works if they live in one place.

## Minimax and negamax

To pick a move, imagine every move, every reply, every reply to that, down to some depth.
Evaluate the positions at the bottom, then work back up assuming each side picks what's best
for it: the side to move takes the maximum, its opponent the minimum. That's **minimax**.

Because chess scores are symmetric (+3 for White is −3 for Black), the two cases fold into
one: a position's score is the best, over all moves, of *minus* the score after the move.
That's **negamax**, and it's what the code below implements.

## Alpha-beta

Most of the minimax tree can't change the answer. Suppose White's first move turns out to be
worth +1. For the second move, the moment Black finds one reply that makes it worse than +1,
White's second move is refuted: White would never play it, so Black's other replies needn't
be examined.

**Alpha-beta** formalises this with a window `[alpha, beta]`:
- `alpha` is what the side to move can already guarantee elsewhere;
- `beta` is the most its opponent will allow.

As soon as a move scores `≥ beta`, the opponent won't let this position happen. The remaining
moves are skipped: a **beta cutoff**.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:negamax}}
```

Alpha-beta returns **exactly** the minimax result. It only skips work that can't change the
answer. With perfect move ordering it searches about `b^(d/2)` positions instead of `b^d`,
which means twice the depth for the same effort.

Try it in the shell on Kiwipete:

```text
> fen r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1
> set alphabeta off
> go depth 4
cb2026 plays Bxa6   (depth 4, +0.10, 4185552 nodes, 868 ms; line: Bxa6 hxg2 Qxg2 bxc3)
> undo
> set alphabeta on
> go depth 4
cb2026 plays Bxa6   (depth 4, +0.10, 131847 nodes, 39 ms; line: Bxa6 hxg2 Qxg2 bxc3)
```

It plays the same move with the same score, from 32 times fewer positions, and that's with
moves in whatever order the generator produced them. Stage 10 orders moves well and gets much
closer to the ideal.

## Mate and draws

- **Checkmate** scores `−MATE + ply` for the side that is mated. Being mated sooner is
  worse, so the winner prefers the quickest mate and the loser the slowest. The score
  translates back to "mate in N" for GUIs.
- **Stalemate, the fifty-move rule, insufficient material and repetition** score 0. Inside the
  search a *single* repetition counts as a draw: if a position can come back once, it can come
  back again, so there's no point searching it twice.

## Stopping

Stage 5 searches to a fixed depth: four plies unless told otherwise. It also stops early when
any of these says so:
- the GUI's `stop`;
- a node limit;
- a time limit, about 1/30th of the remaining clock plus most of the increment.

When stopped early it plays the best move found so far. Stage 6 replaces this with iterative
deepening and proper time management.

## Bench

`cb2026 bench` searches 14 fixed positions to depth 4 and prints the node count:

```text
$ cb2026 bench
Total time (ms) : 108
Nodes searched  : 556884
Nodes/second    : 5113843
```

Because the search is deterministic, that number is a fingerprint of the search. Every commit
message ends with `Bench: <nodes>`, and a change meant to leave the search alone must leave it
unchanged.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 5 vs stage 4 (random mover), 8+0.08, SPRT [0, 5] | +285 −0 =1 over 286 games: passed |

The Elo figure from such a lopsided match is meaningless (about +1100 ± 740), but the point
stands: lookahead plus material counting beats random play every time. From now on each
stage is measured against the one before.

## Further reading

- Chess Programming Wiki: [Minimax](https://www.chessprogramming.org/Minimax),
  [Negamax](https://www.chessprogramming.org/Negamax),
  [Alpha-Beta](https://www.chessprogramming.org/Alpha-Beta),
  [Checkmate](https://www.chessprogramming.org/Checkmate),
  [Point Value](https://www.chessprogramming.org/Point_Value).
- Donald Knuth and Ronald Moore, "An Analysis of Alpha-Beta Pruning", *Artificial
  Intelligence* 6 (1975): the classic proof of how much alpha-beta saves.
