# Iterative deepening and time management

*Stage 6 · tag `stage-06-iterative` · code in `crates/engine/src/lib.rs` and `time.rs`*

Stage 5 searched to a fixed depth. That's fine for a puzzle, but in a real game the engine has a
clock, and it can't know in advance how long depth 7 will take in a given position. Too
shallow wastes time it could have used, and too deep can lose on time. This stage fixes that
with two ideas found in every modern engine:
- **iterative deepening:** search depth 1, then 2, then 3, and so on, until the time is up;
- **time management:** decide, from the clock, how long that is.

## Iterative deepening

Searching depth 1, then depth 2, then depth 3 sounds wasteful, since each search repeats all
the work of the one before. In practice it costs little. Each depth takes several times longer
than the previous one (the *effective branching factor*: between 5 and 15 for cb2026 right
now), so all the shallower searches together add only a fraction of the last one.

In return:

- **There's always an answer.** When time runs out in the middle of depth 8, the engine still
  has the complete result of depth 7 to fall back on.
- **Each depth helps the next.** The best move from depth *d* is searched first at depth
  *d + 1*. Alpha-beta prunes best when the best move comes first (chapter 5), so this ordering
  pays for much of the repeated work. Stages 8 and 10 add a transposition table and better
  move ordering, which use the shallower searches even more, until iterative deepening is
  *faster* than going straight to the final depth.
- **GUIs see progress.** After each depth the engine reports what it found, which is what
  scrolls by in a GUI's analysis pane.

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:search}}
```

When the search is stopped in the middle of a depth, the moves already searched at that depth
are still useful. The first one is last depth's best move, searched deeper, so if any move
finished, the engine plays the best of those. If none finished, it plays the best move from the
previous depth.

The loop also ends early when it finds a forced mate: a mate within *d* plies found by a
full-width search can't be improved by searching deeper.

## Time management

With a clock, the engine has to share its remaining time among all the moves still to play.
cb2026 sets two limits per move:

- **Soft limit:** after finishing a depth, don't start the next one if this much time has gone.
  The next depth would probably take several times longer, so starting it late mostly wastes
  time.
- **Hard limit:** stop at once, even mid-depth. It is the safety net for a depth that takes far
  longer than expected.

```rust,ignore
{{#include ../../crates/engine/src/time.rs:params}}
```

(The fields after `move_overhead` came later, in chapter 6.1.)

```rust,ignore
{{#include ../../crates/engine/src/time.rs:budget}}
```

For example, with 60 seconds left and a 1-second increment:
- the soft limit is (60 s − 30 ms) / 30 + 0.75 s = 2.75 s;
- the hard limit is three times that, 8.25 s.

Neither limit may exceed half the remaining time, so a short clock with a large increment
can't flag. `Move Overhead` (a UCI option, 30 ms by default) is held back for the time the GUI
and operating system take to pass messages. Raise it if cb2026 ever loses on time in a GUI or
on a server.

Here's what that looks like in UCI with `go wtime 60000 btime 60000 winc 1000 binc 1000` from the
starting position:

```text
info depth 5 seldepth 5 score cp 100 nodes 38434 time 5 pv b2b3 b8a6 c1b2 a6b4 b2g7
info depth 6 seldepth 6 score cp -100 nodes 289817 time 40 pv b2b3 b7b6 b1a3 c8a6 g1f3 a6e2
info depth 7 seldepth 7 score cp 100 nodes 1522091 time 194 pv b2b3 b8a6 b1a3 a6b4 a3b5 b4d5 b5a7
info depth 8 seldepth 8 score cp -100 nodes 21698796 time 3096 pv b2b3 b8c6 b1a3 g8f6 g1f3 c6b4 a3b1 b4a2
bestmove b2b3 ponder b8c6
```

Depth 7 finished at 0.19 s, well under the 2.75 s soft limit, so depth 8 started. It finished
at 3.1 s, past the soft limit, so the engine stopped there instead of starting depth 9, which
would have taken far longer. The scores also flip between +1 and −1 from one depth to the
next. That is the **horizon effect**: the search stops in the middle of an exchange and counts
a piece that is about to be recaptured. Stage 7's quiescence search fixes it.

## In the shell

The shell now thinks for one second per move by default and shows each depth as it goes:

```text
> fen r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1
> go
  depth  1    +3.30          48 nodes       0 ms  Bxa6
  depth  2    +0.10         202 nodes       0 ms  Bxa6 bxc3
  depth  3    +3.30        7466 nodes       1 ms  Bxa6 bxc3 Qxf6
  depth  4    +0.10       29664 nodes       5 ms  Bxa6 hxg2 Qxg2 bxc3
  depth  5    +2.30     2105269 nodes     401 ms  Bxa6 hxg2 Rg1 bxc3 Qxf6
  depth  6    -0.90     4734976 nodes     970 ms  Bxa6 bxc3 bxc3 exd5 Nd3 Nxe4
cb2026 plays Bxa6   (depth 6, -0.90, 4734976 nodes, 970 ms)
```

Depth 6 didn't finish within the second. The line shown for it covers only the moves searched
before time ran out, starting with Bxa6.

- `set movetime MS` sets the thinking time;
- `set depth N` switches back to a fixed depth;
- `think off` hides the per-depth lines.

## Pitfalls

- **Checking the clock too often is slow.** Reading the time costs far more than visiting a
  node, so the search checks it every 1024 nodes.
- **Trusting an unfinished depth too much.** Its result is only reliable for the moves it
  finished. That's why the previous best move is always searched first, and why an
  iteration that finished no move is thrown away.
- **Forgetting the overhead.** An engine that plans to use exactly its remaining time
  loses on time, because the GUI measures from when it sent `go` to when it received
  `bestmove`.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 6 vs stage 5, 8+0.08, SPRT [0, 5] | +33 −0 =8 over 41 games: passed (about +390 ± 140 Elo) |

Most of the gain comes from using the clock well: stage 5 always searched to depth 4, which
took a few milliseconds in quiet positions and far too long in sharp ones.

## Further reading

- Chess Programming Wiki: [Iterative Deepening](https://www.chessprogramming.org/Iterative_Deepening),
  [Time Management](https://www.chessprogramming.org/Time_Management),
  [Horizon Effect](https://www.chessprogramming.org/Horizon_Effect).
- David Slate and Larry Atkin, "CHESS 4.5 – The Northwestern University Chess Program",
  in *Chess Skill in Man and Machine* (1977): where iterative deepening became standard.
