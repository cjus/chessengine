# Continuation history

*Milestone M5, after stage 20 · on `main`, no stage tag · code in `crates/engine/src/ordering.rs`*

Chapter 10's history table scores a quiet move by its side, from-square and to-square: it asks
whether the move has caused cutoffs *anywhere* in the tree. But whether a move is good depends
on what just happened. After the opponent's ...Bg4, h3 is often the right reply; after ...Nf6 it
rarely is. History adds up both cases into one number.

**Continuation history** keeps the context. It scores a quiet move *after an earlier move*: the
table is indexed by the earlier move (the piece that moved and its destination) and the candidate
(its piece and destination). cb2026 keeps two such tables:

- **one ply back**, after the opponent's last move. This is a countermove table that scores
  every reply instead of remembering one;
- **two plies back**, after the side to move's own move before that. This one catches plans:
  after Nf3, a knight move to g5 or e5 is often the follow-up, whatever the opponent did in
  between.

Like chapter 14.1's techniques, it came after stage 20, as strength work towards version 1.0,
so no stage tag contains it. Switch it with `set conthist on|off`.

## The tables

A move is described by its piece and destination, which is what the tables are indexed by:

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:piece_to}}
```

There are 12 pieces and 64 squares, so 768 (piece, destination) pairs, and each table has
768 × 768 scores: about 2.4 MB each, as against 32 KB for the history table. The search keeps
the pieces that moved alongside the moves on its path, so at every node it can say what the last
two moves were. At the root, and after a null move, there is no earlier move, and that part of
the score is 0.

## Ordering and updating

A quiet move's ordering key, after the hash move, the captures, the killers and the countermove,
is now the sum of three scores: its history, and its continuation scores after the moves one
and two plies back. Each stays within ±16,384, so the sum stays below the countermove's band.

When a quiet move causes a cutoff, all three tables get the same bonus, `depth²`, and the quiet
moves searched before it the same penalty, with chapter 10's gravity:

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:update}}
```

Between searches every score is halved, as history's is.

The idea is from Stockfish (GPL-3.0), which calls it continuation history; its first versions
called the two tables counter-move history and follow-up history. Stockfish also looks four and
six plies back. The code here is cb2026's own.

## What it saves

The six positions of chapters 10 to 14 at depth 8, with the share of beta cutoffs caused by the
first move searched (the `stats` command shows it):

| Setting | Nodes | vs defaults | First-move cutoffs |
| --- | ---: | ---: | ---: |
| Defaults (continuation history on) | 145,410 | — | 92.7% |
| `set conthist off` | 148,075 | +2% | 92.8% |
| `set history off` | 118,729 | −18% | 93.7% |
| Both off | 112,503 | −23% | 94.3% |

Continuation history takes 2% off the tree. Then the table says something odd: with both history
tables off, the tree is almost a quarter *smaller*, and more of its cutoffs come from the first
move. In chapter 10, before late move reductions, turning history off made the tree 8% bigger.

Since chapter 13, late quiet moves are searched less deeply (LMR) or not at all (late move
pruning). With a worse order, the moves that would have caused a cutoff come late, so they get
reduced or skipped, and the node gives up sooner. The tree shrinks because good moves were
missed, not because they were found quickly. The first-move figure misleads the same way: the
cutoffs that a bad order would have found late are the ones that never happen.

So in a search with late move reductions and pruning, node counts can't say whether move
ordering got better. Only games can. That raises a question about the history table itself:
with continuation history to order the quiet moves, does it still help? An SPRT with history
switched off answers it (below).

## Pitfalls

- **Size and speed.** The tables are large, so most lookups miss the cache. In `bench`,
  cb2026 searches about 3% fewer nodes per second with them. A gain in ordering has to pay for
  that.
- **Slow learning.** A table with 590,000 scores needs many cutoffs before most of them mean
  anything; the history table, with 8,192, learns quickly. Summing them helps: history gives a
  rough order straight away, and continuation history refines it as it learns.
- **Building big tables.** `Box::new([[...]; N])` can build its array on the stack before
  moving it to the heap, and 4.7 MB is more than a search thread's stack. The tables are one
  flat `Vec`, indexed by hand.
- **Which piece moved.** The piece that made the move two plies back may since have been
  captured, so it can't be looked up in the current position. The search records each move's
  piece as the move is made.
- **Nodes aren't Elo.** See above: with heavy pruning, a smaller tree can mean a worse order.

## What it's worth

| Test | Result |
| --- | --- |
| Continuation history vs `main` without it, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+10.7 ± 7.3 Elo** over 4,532 games (+1257 −1117 =2158): passed (LLR 2.96) |

It took 4,532 games: a gain of about 10 Elo sits close to the test's upper bound of 5, and the
closer the true value is to a bound, the longer an SPRT needs to decide. None of the games was
lost on time.

| Test | Result |
| --- | --- |
| History off (continuation history alone orders quiet moves) vs `main` with both, 8+0.08, test Mac (M3 Max), SPRT [−5, 0] | Waiting (cjus/chessengine#73) |

That second test is a simplification test: its bounds ask whether switching history off loses
5 Elo or more. If it passes, the history table adds nothing on top of the rest; if it fails,
history still earns its place.

## Further reading

- Chess Programming Wiki: [History Heuristic](https://www.chessprogramming.org/History_Heuristic),
  [Countermove Heuristic](https://www.chessprogramming.org/Countermove_Heuristic).
- Stockfish's source (GPL-3.0), `movepick.h` and `search.cpp`: the continuation history tables
  and their updates.
