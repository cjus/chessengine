# Spending time where it's needed

*Milestone M5, after stage 20 · on `main`, no stage tag · code in `crates/engine/src/time.rs` and
`lib.rs`*

Chapter 6's time manager gives every move the same budget: a share of the clock, plus most of
the increment. But moves aren't alike. In some, the best move is obvious after a few
milliseconds and every deeper search agrees. In others, the search changes its mind from one
depth to the next, or its score keeps falling as it sees further. A strong player spends little
time on the first kind and a lot on the second, and so can an engine: the search itself shows
which kind of move it is facing.

So after each depth, the soft limit (when to stop starting new depths) is scaled by two
signals. Each has its own switch: `set tmstability` and `set tmscore`.

## Best-move stability

If the best move has stayed the same for several depths in a row, one more depth is unlikely
to change it. If it has just changed, the search hasn't settled, and the next depth may change
it again. The scale, by how many depths in a row the best move has stayed the same:

| Depths unchanged | 0 (just changed) | 1 | 2 | 3 | 4 or more |
| --- | ---: | ---: | ---: | ---: | ---: |
| Soft limit | 180% | 140% | 110% | 90% | 75% |

## A falling score

If the score dropped since the last depth, the search has found a problem, a threat or a line
that doesn't work, and the next depth may find the answer to it. The soft limit grows by 1% per
centipawn the score fell, up to twice as long (100 centipawns). A rising score changes nothing.

## The code

Both scales multiply, and the result is capped at the hard limit:

```rust,ignore
{{#include ../../crates/engine/src/time.rs:adapt}}
```

The iterative deepening loop keeps count of how long the best move has been stable and
compares each depth's score with the last one's:

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:soft_limit}}
```

The hard limit doesn't move, so the clock is exactly as safe as before. Time saved on easy
moves isn't lost either: each move's budget is a share of what remains, so the saving is spent
on later moves. Two cases are left alone. With a fixed `movetime` the GUI asked for that time
exactly. And after a `ponderhit`, the engine already got its thinking time on the opponent's
clock, and it stops after a fixed share of its own.

## What it changes

`ab` can't measure this: it searches to a fixed depth, and `bench` does too, so its node count
doesn't change. In a game it shows in how the time is spread. One self-play game of 60 moves
each at 8+0.08, before and after, measured twice: on the engine of chapter 14.1 before
continuation history, and again with continuation history and singular extensions (each pair
is two different games, so the positions differ):

| | Mean per move | Median | Slowest 10% from | Longest |
| --- | ---: | ---: | ---: | ---: |
| Before (fixed soft limit) | 210 ms | 146 ms | 527 ms | 857 ms |
| After | 210 ms | 169 ms | 433 ms | 648 ms |
| Before, with singular extensions | 210 ms | 121 ms | 467 ms | 916 ms |
| After, with singular extensions | 210 ms | 156 ms | 459 ms | 851 ms |

The total is the same, as it must be: both use their clock. The typical think gets longer and
the longest ones a little shorter. One game is a small sample, and the two pairs differ in how
much. The games say it plays better, by about **33 Elo** (see "What it's worth"): spending the
same time in different places is worth as much as singular extensions.

## Pitfalls

- **Never past the hard limit.** Scaling the soft limit up is safe only because the hard limit
  still stops the search mid-depth. Scaling the hard limit too would risk the clock.
- **Scores wobble.** Aspiration windows, and the odd/even effect (a search that ends on the
  side to move's ply tends to score a little higher than one that ends on the opponent's),
  move the score by a few centipawns from one depth to the next with nothing wrong. At 1% per
  centipawn that costs a few percent of time; a steeper scale would chase noise.
- **Respect what the GUI asked for.** `go movetime 500` means 500 ms. A time manager that
  stops early "because the move is obvious" breaks it.
- **The numbers are a starting point.** The scale's values are reasonable, not tuned. Like the
  rest of the time manager, they are named parameters, and an SPRT at one time control says
  little about another.

## What it's worth

| Test | Result |
| --- | --- |
| Adaptive soft limit vs `main` without it (with singular extensions), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+33.5 ± 14.3 Elo** over 1,062 games (+299 −197 =566): passed (LLR 2.97), no time losses |

The gain was measured at one time control, 8+0.08. At longer ones each depth takes longer and
the trade-offs shift, so it is worth checking there too (`scripts/mac-sprt.sh <PR> 0 5
40+0.4`).

## Further reading

- Chess Programming Wiki: [Time Management](https://www.chessprogramming.org/Time_Management).
- Robert Hyatt, "Using Time Wisely", *ICCA Journal* 7 (1984).
