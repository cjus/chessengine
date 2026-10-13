# Extensions and reductions

*Milestone M5, after stage 20 · on `main`, no stage tag · code in `crates/engine/src/search.rs`*

Chapters 12 to 14 made the tree smaller by guessing which moves and positions don't need a full
search. This chapter moves depth in both directions. An **extension** searches a move *deeper*
than its siblings; a **reduction** searches a whole node *shallower* (chapter 13's late move
reductions work on single moves). One fact about a position, whether the side to move is
**improving**, changes how boldly the pruning of chapters 13 and 14 guesses. And one fact about
a move, its **history** score, changes how much late move reductions take off it.

The techniques in this chapter came after stage 20, as strength work towards version 1.0
(milestone M5), so no stage tag contains them: read this chapter against `main`. Like everything
before them, each can be switched off and on: `set checkext`, `set iir`, `set improving` and
`set histlmr`.

## Check extensions

A move that gives check is searched one ply deeper than the moves around it. A check leaves the
opponent few replies, so the extra ply costs little, and checks are where the horizon hides
mates and lost pieces: a mating attack is a run of checks, and stopping halfway through one
evaluates a position whose point hasn't happened yet.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:check_extension}}
```

The child is then searched at `depth - 1 + extension`. The `ply < MAX_PLY / 2` condition
matters: an extension that can apply at every ply never reaches the horizon, and a long series
of checks would grow the search without bound.

Philidor's legacy shows what it buys. White mates in four, and every move is a check: 1. Nf7+
Kg8 2. Nh6+ Kh8 3. Qg8+ Rxg8 4. Nf7#.

```text
> fen 4r2k/6pp/8/3Q2N1/8/8/6PP/6K1 w - - 0 1
> ab checkext 8
```

With the extension the engine sees the mate at depth 8; without it, it needs a deeper search.

**cb2026 ships with it off.** Extending every check made the bench search about 24% bigger, and
an SPRT with the extension on against the same engine without it found no gain (see "What it's
worth" below). Most of what it finds is found anyway: late move reductions never reduce a move
that gives check (chapter 13), and the quiescence search resolves the captures. Strong engines
extend more selectively, for example only checks that don't lose material, or only **singular**
moves: a move that a reduced search, with that move left out, shows to be much better than every
alternative. The switch stays, so the experiment can be repeated.

## Internal iterative reductions

The hash move, the best move the transposition table remembers from an earlier search of the
position, is usually the best move, and searching it first is what makes alpha-beta fast
(chapter 10). A node with no hash move has never been searched, every search of it failed low,
or its entry has been replaced. Its move ordering will be poor, so a full-depth search of it is
expensive. And nodes like that are rarely on the principal variation, which every iteration
searches first and leaves in the table.

**Internal iterative reduction** (IIR) searches such a node, from depth 4 on, one ply shallower:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:iir}}
```

If the node does matter, little is lost: the next iteration of iterative deepening reaches it
again, now with the move this search stored, and searches it at full depth.

The older technique, **internal iterative deepening** (IID), did more work instead of less:
before searching a node without a hash move, it ran a shallower search of the same node just to
find a move to try first. IIR gets that move from the next iteration instead, and is cheaper and
about as good. Stockfish (GPL-3.0) has replaced IID with it.

## Improving

A side whose static evaluation is higher than two plies ago, the last time it was to move, is
**improving**: its moves are working, and its evaluation will probably keep rising.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:improving}}
```

That changes the bet reverse futility pruning makes.

**Reverse futility pruning** (chapter 14) bets that a node far above beta stays there. If the
side to move is improving, the bet is safer, so the margin shrinks by half a ply's worth
(`rfp_improving`, 40 centipawns):

```rust,ignore
{{#include ../../crates/engine/src/search.rs:reverse_futility}}
```

**Late move pruning** (chapter 13) could use it too. In an improving position quiet moves are
more often good, so it could try more of them before giving up. That is the parameter
`lmp_improving`, and it is 1: no change.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:lmp}}
```

This is the third version. **The first failed.** It shrank the margin by a whole ply, as
Stockfish 11 (GPL-3.0) did. At depth 1 that left no margin at all: the node was cut off whenever
its static evaluation alone reached beta, and cb2026's evaluation, unlike Stockfish's, knows
nothing about pieces under attack. In an SPRT against the same engine without it, cb2026 lost
about 32 Elo (see "What it's worth"). **The second failed too**, by about 18 Elo. It took half a
ply off the margin, as here, so at depth 1 a node still needs 40 centipawns to spare. Both also
doubled late move pruning's threshold when improving (`lmp_improving` = 2). The version above
leaves that out, so it tests the margin on its own.

**The third version failed as well**, by about 33 Elo, so **cb2026 ships with improving off**.
Taken together, the three results point at reverse futility pruning: with the margin alone
smaller, the engine lost more than with the same margin and late move pruning doubled, so the
late move pruning part seems to have made up some of the loss. Testing the parts one at a time
is what showed that. The switch and both parameters stay, so either part can be tried again.

Both tests also ran with a bug, found afterwards: the root is searched by its own function,
which didn't store its static evaluation, so every node two plies down compared its evaluation
with `-INFINITY` and counted as improving. Fixing it changed the six test positions' tree by 31
nodes, so it is probably not why improving lost.

## History in late move reductions

Late move reductions (chapter 13) reduce a quiet move by how late it comes in the ordering. But
that order is only as good as the history table, which ranks the quiet moves, and it puts the
killers and the countermove first whatever their history. A move with a high history score
that still came late has caused cutoffs elsewhere, and it deserves a closer look.

So the reduction shrinks by a ply for every 256 points of history score
(`lmr_history_divisor`), by at most two plies (`lmr_history_max`). A negative score makes it
grow the same way.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:lmr}}
```

The divisor comes from measuring. History scores can run from −16,384 to +16,384, but those of
the moves LMR actually reduces are mostly tiny. Over a 40-move self-play game at 200 ms a move
(measured before continuation history, chapter 10.1, came in):

| History score of a reduced move | Share |
| --- | ---: |
| −256 or below | almost none |
| −256 to −64 | 9% |
| −64 to +64 | 74% |
| +64 to +256 | 7% |
| +256 to +512 | 3% |
| +512 and above | 7% |

So this version reduces about one late move in ten less, and hardly ever reduces one more. The
scores lean positive because the penalties go only to the quiet moves searched before a quiet
move's cutoff. At a node where no quiet move cuts off, which is most of the nodes LMR works on,
none of them is penalised.

Stockfish (GPL-3.0) adjusts its reductions by history too, using its history and continuation
history added up, on a scale of its own; the scale here is cb2026's. Continuation history
(chapter 10.1) could join the score in the same way.

## What it saves

The six positions of chapters 10 to 14 (the first six bench positions) at depth 8:

| Setting | Nodes | vs defaults |
| --- | ---: | ---: |
| Defaults (IIR, continuation history and history in LMR on; improving and the check extension off) | 145,132 | — |
| `set histlmr off` | 145,410 | +0.2% |
| `set iir off` | 196,436 | +35% |
| `set improving on` | 117,842 | −19% |
| `set checkext on` | 151,089 | +4% |

IIR takes about a quarter off the tree. Improving takes off almost a fifth more; its second
version, which also made late move pruning wait longer, took off 7%, and its first 31%. All
three made the engine weaker: the games below decide, not the node counts.

History in LMR hardly changes these trees. History scores grow with depth (the bonus is
`depth²`) and over a game (each search keeps half of the last one's), and a search to depth 8
from empty tables leaves few moves at 256 or more. The bench, which searches to depth 10, grew
1.7%. In the self-play game above, where the tables carry over from move to move, one reduced
move in ten was affected.

## Pitfalls

- **Search explosion.** An extension that can apply at every ply never reaches the horizon, and
  one that applies often can double the tree. Limit extensions (here, to the first half of the
  maximum ply), and count nodes as well as games.
- **Overlap.** Every technique here works on top of all the others. The check extension found
  little because LMR already spares checks; turning a technique off shows only what it adds *on
  top of* the rest.
- **Store what you searched.** IIR replaces `depth` for the rest of the node, so the table entry
  records the reduced depth. Storing the original depth would let the table answer later, deeper
  searches with a result it doesn't have.
- **Nodes aren't Elo.** IIR saves nodes by searching some nodes less deeply. Whether that costs
  more than it saves is for an SPRT to say, not a node count.
- **A margin that reaches zero.** A margin scaled down by a flag can disappear altogether: at
  depth 1, the first version of improving took reverse futility pruning's margin to zero. Make
  sure every margin stays positive at the smallest depth it applies to; a unit test now checks
  that `rfp_improving` stays below `rfp_margin`.
- **Nodes aren't Elo, again.** The first version of improving made the tree almost a third
  smaller and lost 32 Elo. A big node saving is a reason to run an SPRT, not a result.
- **Every ply stores its evaluation, the root included.** A heuristic that looks back two plies
  reads what earlier nodes stored. A node searched by a different function, like the root here,
  is easy to miss, and nothing fails: the comparison just gives the wrong answer. A unit test
  now checks the root's entry.
- **Measure the scale before choosing a divisor.** A history score's range says nothing about
  where the scores actually sit. Here they sit within ±64 for three moves in four, so a divisor
  set from the range (say 8,192, two plies at the cap) would almost never change a reduction.
- **Two plies back may be a check.** In check, cb2026 computes no static evaluation (it is
  recorded as `-INFINITY`), so a side that was in check two plies ago always counts as
  improving. Some engines look four plies back in that case.

## What it's worth

| Test | Result |
| --- | --- |
| Check extensions on vs off (stage 20), 8+0.08, cloud, SPRT [0, 5] | −8.3 ± 17.7 Elo over 840 games (+229 −249 =362), no verdict when the session's two-hour limit stopped it: no gain, so it is off by default |
| IIR vs `main` without it, 8+0.08, test Mac, SPRT [0, 5] | **+19.2 ± 10.4 Elo** over 2,337 games (+705 −576 =1056): passed |
| Improving, first version (a whole ply off the margin), vs `main` without it (with IIR), 8+0.08, test Mac (M1 Max), SPRT [0, 5] | **−32.0 ± 15.1 Elo** over 1,142 games (+267 −372 =503): failed (LLR −2.96) |
| Improving, half a ply off the margin, late move pruning doubled, vs `main` without it (with IIR), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **−17.7 ± 11.5 Elo** over 1,691 games (+362 −448 =881): failed (LLR −2.97) |
| Improving, half a ply off the margin only, vs `main` without it (with IIR), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **−33.0 ± 15.3 Elo** over 1,036 games (+221 −319 =496): failed (LLR −2.96), so improving is off by default |
| History in LMR vs `main` without it, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | Waiting (cjus/chessengine#70) |

## Further reading

- Chess Programming Wiki: [Extensions](https://www.chessprogramming.org/Extensions),
  [Check Extensions](https://www.chessprogramming.org/Check_Extensions),
  [Singular Extensions](https://www.chessprogramming.org/Singular_Extensions),
  [Internal Iterative Deepening](https://www.chessprogramming.org/Internal_Iterative_Deepening),
  [Internal Iterative Reductions](https://www.chessprogramming.org/Internal_Iterative_Reductions),
  [Improving](https://www.chessprogramming.org/Improving).
