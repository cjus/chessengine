# Move generation and perft

*Stage 2 · tag `stage-02-movegen` · code in `crates/chess`*

An engine spends much of its time asking "what are the legal moves here?". Getting the answer
exactly right is the first hard part of writing one, and **perft** is how we prove we did.

## Attacks

Everything starts with attacks: which squares does each piece attack?

### Leaping pieces: tables

A knight on g1 attacks e2, f3 and h3 whatever else is on the board. So the attacks of knights,
kings and pawns are computed once, at compile time, into 64-entry tables:

```rust,ignore
{{#include ../../crates/chess/src/attacks.rs:leapers}}
```

### Sliding pieces: walking rays

Bishops, rooks and queens slide until they hit something, so their attacks depend on the
occupied squares. Stage 2 computes them the obvious way: step out in each direction and stop
after the first occupied square (which is attacked either way: an enemy piece can be captured,
an own piece is defended).

```rust,ignore
{{#include ../../crates/chess/src/attacks.rs:slider}}
```

This is simple and clearly correct, which makes it the reference that stage 3's much faster
*magic bitboards* are tested against.

### Who attacks a square?

A neat trick turns "what does this piece attack?" around into "what attacks this square?":
put each kind of piece on the square and see which enemy pieces of that kind it would attack.
A knight on e4 attacks exactly the squares from which a knight attacks e4.

```rust,ignore
{{#include ../../crates/chess/src/movegen.rs:attackers_to}}
```

## Legal moves, directly

The simplest generator makes *pseudo-legal* moves (each piece's moves, ignoring checks), plays
each one and throws away those that leave the king in check. cb2026 instead generates only
legal moves, using two facts about the position:

- **Checkers.** In double check only the king can move. In single check, every other move must
  capture the checker or block between it and the king: a *check mask* of allowed destinations.
- **Pins.** A piece standing alone between its king and an enemy slider may only move along
  that line.

```rust,ignore
{{#include ../../crates/chess/src/movegen.rs:generate}}
```

Note the line that lifts the king off the board before testing its destination squares.
Without it, a king in check from a rook on the same rank could "escape" one square further
along the rank, because the king itself would seem to block the rook.

### The awkward cases

Two moves are rare and subtle, so cb2026 checks them by playing them on a copy and looking at
the king:

- **En passant** removes *two* pawns from one rank, which can expose the king to a rook on that
  rank even though neither pawn was pinned on its own.
- **Castling** in Chess960, where the castling rook itself might be what shields the king.

Castling gets one more idea: the move is stored as "king takes own rook" (`e1h1`). That
describes Chess960 castling unambiguously, even when the king doesn't move at all, and the
same code then handles standard chess.

```rust,ignore
{{#include ../../crates/chess/src/movegen.rs:castling}}
```

## Making moves

`make_move` updates the board and everything that depends on the move:
- captures, including en passant;
- promotions;
- castling rights, which are lost when a king or castling rook moves or a castling rook is
  captured;
- the en passant square;
- both clocks.

```rust,ignore
{{#include ../../crates/chess/src/makemove.rs:make_move}}
```

To try a move and come back, cb2026 copies the position first ("copy-make"). A position is a
few hundred bytes, so copying is cheap and there's no undo code to get wrong.

One detail: the en passant square is recorded only when an enemy pawn could actually capture
there. Then two positions that are really the same always look the same, which matters once
positions are compared and hashed (stage 4).

## Perft: proving it

**Perft** counts the leaf positions of the full game tree to a given depth: every legal move,
every reply, and so on.

```rust,ignore
{{#include ../../crates/chess/src/perft.rs:perft}}
```

The counts for standard test positions have been reproduced by many independent programs.
A move generator that matches them exactly at every depth is almost certainly right, because
any bug changes the count. When a count is wrong, `divide` splits it by first move, which
points straight at the move whose subtree is wrong.

cb2026's suites live in `tests/data/`:
- `perft.epd`: the Chess Programming Wiki positions, including "Kiwipete", plus small positions
  aimed at classic bugs. Those bugs include an illegal en passant, castling that gives check,
  and underpromotion.
- `perft960.epd`: Chess960 start and middlegame positions.

The unit tests run every depth up to a million nodes. CI runs every depth in a release build
(`cargo run --release -p cb2026-tools --bin perft-suite`), about 666 million positions.

**Bulk counting.** At depth 1 the count is just the number of legal moves, so they need not
be played. That shortcut alone makes perft several times faster.

## Try it

```text
> perft 5
Depth 5: 4865609 nodes in 0.030 s (160.0 million nodes/s)   # speed varies by machine
> e2e4
> legal
> divide 2
> fen r3k2r/8/8/8/2b5/8/8/R3K2R w KQkq - 0 1
> legal          # no e1g1: the bishop on c4 covers f1
> bb pinned
> attackers f1
```

## What it's worth

Still nothing in Elo: there's no search yet. But every later stage relies on this code being
exactly right.

## Further reading

- Chess Programming Wiki: [Move Generation](https://www.chessprogramming.org/Move_Generation),
  [Pin](https://www.chessprogramming.org/Pin),
  [Classical Approach](https://www.chessprogramming.org/Classical_Approach),
  [Perft](https://www.chessprogramming.org/Perft),
  [Perft Results](https://www.chessprogramming.org/Perft_Results),
  [Chess960 Perft Results](https://www.chessprogramming.org/Chess960_Perft_Results).
