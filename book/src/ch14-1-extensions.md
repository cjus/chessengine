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
before them, each can be switched off and on: `set checkext`, `set singular`, `set multicut`,
`set iir`, `set improving` and `set histlmr`.

## Check extensions

A move that gives check is searched one ply deeper than the moves around it. A check leaves the
opponent few replies, so the extra ply costs little, and checks are where the horizon hides
mates and lost pieces: a mating attack is a run of checks, and stopping halfway through one
evaluates a position whose point hasn't happened yet.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:check_extension}}
```

The child is then searched at `depth - 1 + extension`. (`singular_move` is the next
section's.) The `ply < MAX_PLY / 2` condition
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

## Singular extensions

The check extension extends a kind of move. A **singular extension** extends a move because of
how it compares with the others: the hash move, if it is much better than every alternative. A
move like that is probably forced, a recapture or the one defence to a threat, and a mistake
anywhere in its line costs the most, so it is worth a ply more.

Finding out costs a search. Before the moves are searched, the node is searched once more, at
half the depth, **without the hash move**, with a null window just below the hash move's score:
`singular_beta`, the hash table's score less 2 centipawns per ply of depth. If no other move
reaches `singular_beta`, the hash move is singular and is searched with one ply more.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:singular}}
```

The test needs a hash entry worth comparing against: a lower bound or an exact score, at most 3
plies shallower than the node. It applies from depth 8 on, so near the root and only in deep
searches. And no extension is made beyond twice the root depth, so that a chain of them can't
grow the search without limit.

The same search answers a second question for free. If another move does reach
`singular_beta`, and `singular_beta` is at least beta, then two moves beat beta: the hash move,
and this one even at half the depth. The node is cut off on the spot. That is **multi-cut**.

The search without the hash move asks a different question from the node's own search, so it
must not mix with it. It neither reads nor writes the transposition table (an entry about the
node with all its moves would answer the wrong question, and its result would poison the
entry), it tries no null move, and IIR doesn't reduce it further. If the hash move is the only
legal move, the search has nothing to search: it fails low, since nothing else comes close.

Singular extensions go back to Deep Thought, the Carnegie Mellon machine that led to Deep Blue
(Anantharaman, Campbell and Hsu, 1988). This version, and multi-cut, are as in Stockfish (GPL-3.0); the code is
cb2026's own.

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

So the reduction shrinks by a ply for every `lmr_history_divisor` points of the move's score, by
at most two plies (`lmr_history_max`). A negative score makes it grow the same way. The score
is the one that ordered the quiet moves, history plus continuation history; the first version
used history alone.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:lmr}}
```

### The first version

The first version used the history score alone, and a divisor of 256. The divisor came from
measuring. History scores can run from −16,384 to +16,384, but those of
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

So that version reduces about one late move in ten less, and hardly ever reduces one more. The
scores lean positive because the penalties go only to the quiet moves searched before a quiet
move's cutoff. At a node where no quiet move cuts off, which is most of the nodes LMR works on,
none of them is penalised.

Stockfish (GPL-3.0) adjusts its reductions by history too, using its history and continuation
history added up, on a scale of its own; the scale here is cb2026's.

**The first version had no verdict.** An SPRT against the same engine without it found no
clear difference: about +2 Elo, give or take 5, after 8,910 games (see "What it's worth"). An
effect that small sits between the test's two hypotheses, 0 and 5 Elo, which is where an SPRT
takes longest; it could have needed tens of thousands more games, so the test was stopped.
That fits the table above: the adjustment changed about one reduction in ten, by a ply or two,
and almost never made one bigger.

### The second version

The second version scores a move as the move picker does, history plus continuation history
(`OrderingTables::quiet_score`), so the reduction and the order agree about which moves look
good. Adding continuation history doesn't widen the scores much at the moves LMR reduces. Over
the same kind of game, after continuation history came in:

| History plus continuation history of a reduced move | Share |
| --- | ---: |
| −256 or below | 0.5% |
| −256 to −128 | 5% |
| −128 to +128 | 86% |
| +128 to +256 | 3% |
| +256 and above | 6% |

So the divisor comes down to 128. Then about one reduced move in seven changes: about 9% are
reduced less and 5% more. For the first time the adjustment works both ways: a move with a
bad score, one that failed where it was tried before, is reduced a ply more.

## What it saves

The six positions of chapters 10 to 14 (the first six bench positions) at depth 8:

| Setting | Nodes | vs defaults |
| --- | ---: | ---: |
| Defaults (IIR, continuation history and history in LMR on; improving and the check extension off) | 148,427 | — |
| `set histlmr off` | 145,410 | −2% |
| `set iir off` | 194,771 | +31% |
| `set improving on` | 117,842 | −21% |
| `set checkext on` | 152,496 | +3% |

IIR takes about a quarter off the tree. Improving takes off over a fifth more; its second
version, which also made late move pruning wait longer, took off 7%, and its first 31%. All
three made the engine weaker: the games below decide, not the node counts.

History in LMR's first version hardly changed these trees: history scores grow with depth (the
bonus is `depth²`) and over a game (each search keeps half of the last one's), and a search to
depth 8 from empty tables leaves few moves at 256 or more. The second version, with half the
divisor and both directions, makes the tree 2% bigger here, and the bench 2.1% bigger.

Singular extensions don't appear here: at depth 8, no node below the root has 8 plies left. At
depth 10 they make the tree **85% bigger** (694,595 nodes against 375,334). Over the six
positions the search tried 472 hash moves, extended 120 and cut off 138 nodes by multi-cut;
without multi-cut the tree is 7% bigger again. That is the price of extending near the root:
each extension adds a ply to a large part of the tree. The games say it is worth it: about **+30
Elo** (see "What it's worth"), the biggest gain of anything in this chapter, with a tree almost
twice the size. Nodes aren't Elo, in this direction too.

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
- **A second search of the same node.** The search without the hash move runs at the same ply
  as the node itself. Anything stored per ply or per position, the hash table above all, has to
  know that it is a different search, or it will mix the two results.
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
| History in LMR vs `main` without it (with continuation history), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | +2.1 ± 5.0 Elo over 8,910 games (+2188 −2134 =4588): no verdict (LLR −0.30), stopped; off by default |
| Singular extensions and multi-cut vs `main` without them (with continuation history), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+29.5 ± 13.3 Elo** over 1,357 games (+409 −294 =654): passed (LLR 2.96) |
| History in LMR, second version (history plus continuation history, divisor 128), vs `main` without it, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | Waiting (cjus/chessengine#74) |

## Further reading

- Chess Programming Wiki: [Extensions](https://www.chessprogramming.org/Extensions),
  [Check Extensions](https://www.chessprogramming.org/Check_Extensions),
  [Singular Extensions](https://www.chessprogramming.org/Singular_Extensions),
  [Internal Iterative Deepening](https://www.chessprogramming.org/Internal_Iterative_Deepening),
  [Internal Iterative Reductions](https://www.chessprogramming.org/Internal_Iterative_Reductions),
  [Improving](https://www.chessprogramming.org/Improving).
