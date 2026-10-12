# Extensions and reductions

*Milestone M5, after stage 20 · on `main`, no stage tag · code in `crates/engine/src/search.rs`*

Chapters 12 to 14 made the tree smaller by guessing which moves and positions don't need a full
search. This chapter moves depth in both directions. An **extension** searches a move *deeper*
than its siblings; a **reduction** searches a whole node *shallower* (chapter 13's late move
reductions work on single moves). And one fact about a position, whether the side to move is
**improving**, changes how boldly the pruning of chapters 13 and 14 guesses.

The techniques in this chapter came after stage 20, as strength work towards version 1.0
(milestone M5), so no stage tag contains them: read this chapter against `main`. Like everything
before them, each can be switched off and on: `set checkext`, `set iir` and `set improving`.

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

That changes the bets two pruning techniques make.

**Reverse futility pruning** (chapter 14) bets that a node far above beta stays there. If the
side to move is improving, the bet is safer, so the margin shrinks by one ply's worth:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:reverse_futility}}
```

**Late move pruning** (chapter 13) bets that once enough quiet moves have failed, the rest will
too. In an improving position quiet moves are more often good, so it tries twice as many before
giving up:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:lmp}}
```

The first change prunes more and the second less. Together they made the tree almost a third
smaller on the six test positions (below), and the engine weaker: in an SPRT against the same
engine without them, cb2026 lost about 32 Elo (see "What it's worth").

**cb2026 ships with it off.** The likely cause, not yet tested, is the first change at depth 1.
There the margin becomes zero, so the node is cut off whenever its static evaluation alone
reaches beta, and the static evaluation knows nothing about pieces under attack. Stockfish
(GPL-3.0) uses the improving idea in many more of its pruning and reduction decisions, so the
idea can work; this version of it didn't. The switch stays, so a gentler version can be tried:
one that changes only late move pruning, or never lets the margin reach zero.

## What it saves

The six positions of chapters 10 to 14 (the first six bench positions) at depth 8:

| Setting | Nodes | vs defaults |
| --- | ---: | ---: |
| Defaults (IIR on; improving and the check extension off) | 148,075 | — |
| `set iir off` | 193,361 | +31% |
| `set improving on` | 101,865 | −31% |
| `set checkext on` | 146,051 | −1% |

IIR takes almost a quarter off the tree, and turning improving on takes almost a third off what
is left. Only IIR made the engine stronger: the games below decide that, not the node counts.

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
  depth 1, improving took reverse futility pruning's margin to zero. Make sure every margin
  stays positive at the smallest depth it applies to.
- **Nodes aren't Elo, again.** Improving made the tree almost a third smaller and lost 32 Elo. A
  big node saving is a reason to run an SPRT, not a result.
- **Two plies back may be a check.** In check, cb2026 computes no static evaluation (it is
  recorded as `-INFINITY`), so a side that was in check two plies ago always counts as
  improving. Some engines look four plies back in that case.

## What it's worth

| Test | Result |
| --- | --- |
| Check extensions on vs off (stage 20), 8+0.08, cloud, SPRT [0, 5] | −8.3 ± 17.7 Elo over 840 games (+229 −249 =362), no verdict when the session's two-hour limit stopped it: no gain, so it is off by default |
| IIR vs `main` without it, 8+0.08, test Mac, SPRT [0, 5] | **+19.2 ± 10.4 Elo** over 2,337 games (+705 −576 =1056): passed |
| Improving vs `main` without it (with IIR), 8+0.08, test Mac, SPRT [0, 5] | **−32.0 ± 15.1 Elo** over 1,142 games (+267 −372 =503): failed (LLR −2.96), so it is off by default |

## Further reading

- Chess Programming Wiki: [Extensions](https://www.chessprogramming.org/Extensions),
  [Check Extensions](https://www.chessprogramming.org/Check_Extensions),
  [Singular Extensions](https://www.chessprogramming.org/Singular_Extensions),
  [Internal Iterative Deepening](https://www.chessprogramming.org/Internal_Iterative_Deepening),
  [Internal Iterative Reductions](https://www.chessprogramming.org/Internal_Iterative_Reductions),
  [Improving](https://www.chessprogramming.org/Improving).
