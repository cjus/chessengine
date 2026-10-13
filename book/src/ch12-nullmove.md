# Null-move pruning

*Stage 12 · tag `stage-12-nullmove` · code in `crates/engine/src/search.rs`*

Everything so far has been **sound**: with alpha-beta, the transposition table, move ordering
and PVS, cb2026 finds the same score minimax would, only faster. From this chapter on, the
search starts taking calculated risks. It prunes moves that are *probably* irrelevant,
accepting the occasional mistake in exchange for searching much deeper. Null-move pruning
was the first of these techniques to become universal, and it is still one of the most
effective.

## The idea

In chess, having the move is almost always an advantage. Imagine the side to move could pass,
a **null move**, and the opponent then gets a full search to show what they can do. If even
after passing the score stays at or above beta, then a real move would surely do at least as
well. The opponent won't allow this position, and the node can be cut off without searching a
single real move.

The test is cheap for two reasons:

- the null move's search is **reduced**: 3 plies less than normal, and one more for every 6
  plies of depth;
- it uses a **null window** around beta, since it only asks "still ≥ beta?".

```rust,ignore
{{#include ../../crates/engine/src/search.rs:null_move}}
```

(The `excluded` condition came later, with singular extensions in chapter 14.1.)

Most positions where the side to move is far ahead fail this test for the opponent quickly,
and whole subtrees vanish. On the six test positions at depth 8, null-move pruning cuts the
nodes from 40.6 million to 10.0 million, a 75% saving. That's roughly two extra plies of
depth in the same time.

## When passing would be better: zugzwang

The idea rests on one assumption: that moving is better than passing. In **zugzwang** it is
false: every legal move makes the position worse, and the side to move would love to pass. In
such positions null-move pruning cuts off nodes that are actually lost.

Zugzwang is rare in the middlegame and common in pawn endings, so cb2026 doesn't try a null
move when the side to move has only pawns (and its king). Stronger engines add a
**verification search** for deep nodes, which re-searches without null moves to confirm a
cutoff. cb2026 may add this later if testing shows it helps.

## The other conditions

A null move is tried only when all of these hold:

- **Not in check:** passing would leave the king in check, which is illegal even in this
  imaginary world.
- **Not twice in a row:** two passes would cancel out and just waste a reduced search.
- **Not on the principal variation:** at full-window nodes the exact score matters and a
  risky cutoff could corrupt the main line, so null moves are only tried at null-window
  nodes. This is why null-move pruning needs PVS (chapter 11).
- **Enough depth left:** at least 3 plies.
- **The static evaluation is already ≥ beta:** if the side to move isn't even ahead before
  passing, the test is unlikely to succeed and costs nodes for nothing.

The chess crate's `make_null_move` passes the turn. It clears the en passant square and resets
the halfmove clock, so the repetition check doesn't treat positions on either side of the pass
as repetitions.

A mate found after a null move isn't real (the side that passed broke the rules), so a
null-move cutoff never claims a mate score; it returns beta instead.

## Pitfalls

- **Zugzwang.** Covered above. If cb2026 misplays a pawn ending, `set nullmove off` is the
  first thing to try.
- **Threats beyond the reduced depth.** The search after the pass is shallower than a
  normal one, so it can miss a deep threat and cut off a node that needed searching. These
  errors are the price of the extra depth, and the SPRT shows whether the trade is worth it.
- **Test positions.** Many "mate in N" puzzle solutions involve quiet moves that null-move
  pruning makes harder to find, so engines can do worse on puzzle suites while playing
  stronger games.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 12 vs stage 11, 8+0.08, SPRT [0, 5] | +155 −39 =61 over 255 games: passed (about +170 ± 40 Elo) |

## Further reading

- Chess Programming Wiki: [Null Move Pruning](https://www.chessprogramming.org/Null_Move_Pruning),
  [Null Move Observation](https://www.chessprogramming.org/Null_Move_Observation),
  [Zugzwang](https://www.chessprogramming.org/Zugzwang).
- Chrilly Donninger, "Null Move and Deep Search: Selective-Search Heuristics for Obtuse Chess
  Programs", *ICCA Journal* 16 (1993): the adaptive reduction.
- Omid David and Nathan Netanyahu, "Verified Null-Move Pruning", *ICGA Journal* 25 (2002).
