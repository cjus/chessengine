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
before them, each can be switched off and on: `set checkext`, `set singular`, `set doubleext`,
`set multicut`, `set negext`, `set iir`, `set improving` and `set histlmr`.

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
`singular_beta`, the hash table's score less 3 centipawns per ply of depth. If no other move
reaches `singular_beta`, the hash move is singular and is searched with one ply more.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:singular}}
```

The test needs a hash entry worth comparing against: a lower bound or an exact score, at most 3
plies shallower than the node (`singular_tt_depth_margin`, see below). It applies from depth 4 on (`singular_min_depth`), so only
with a few plies of the search still to come. And no extension is made beyond twice the root
depth, so that a chain of them can't grow the search without limit.

The first version started at depth 8. That is cautious: Stockfish (GPL-3.0) starts lower. Each
test costs a search at half the depth, so starting lower runs many more of them, and the
extensions and multi-cuts they find change the tree in both directions. The bench and the six
positions of "What it saves" at depth 10, by the depth singular extensions start from:

| Start from depth | Bench | Six positions at depth 10 | Tests | Extended | Multi-cuts |
| --- | ---: | ---: | ---: | ---: | ---: |
| 8 (first version) | 1,100,720 | 668,426 | 485 | 125 | 142 |
| 7 | 1,031,076 | 630,754 | 808 | 236 | 282 |
| 6 (second version) | 1,131,028 | 647,169 | 1,738 | 708 | 635 |
| 5 | 1,134,904 | 722,245 | 3,436 | 1,371 | 1,413 |
| 4 | 1,343,312 | 750,416 | 6,449 | 3,107 | 2,484 |

Starting two plies lower runs the test more than three times as often, but the trees hardly
grow, and at depth 10 they shrink: the extra tests find multi-cuts as well as extensions, and
the two pull in opposite directions. (These counts include negative extensions, below. Before
them, starting from depth 6 made the depth-10 tree 4% bigger, not 3% smaller.) Node counts
don't say which plays better, so depth 6, the bigger step towards Stockfish's, went to the
games: about **+5 Elo** (see "What it's worth").

That suggested going lower still, and the next two-ply step is depth 4, close to where
Stockfish starts. From there the trees do grow: the test runs almost four times as often as
from depth 6, the bench is 19% bigger and the depth-10 tree 16%. The evidence gets thinner,
too. At depth 4 the test is a search of a single ply, against an entry that may come from a
search of one ply, so "nothing else comes close" means less. The games say the many extra
extensions and cutoffs are worth it all the same: about **+6 Elo** more (see "What it's
worth").

One effect shows already. The check extension's test position above, Philidor's legacy, is a
chain of forced moves, each the only good one. With IIR off, singular extensions from depth 4
find them singular and extend them, so at depth 7 the engine finds the mate with the check
extension off too; from depth 6 it doesn't. (With IIR on, the defaults, neither finds it at
depth 7 or 8 without the check extension.) Where it applies, the more general extension does
the special one's job. The unit test for the check extension now switches singular extensions
off, as it does IIR.

The same search answers a second question for free. If another move does reach
`singular_beta`, and `singular_beta` is at least beta, then two moves beat beta: the hash move,
and this one even at half the depth. The node is cut off on the spot. That is **multi-cut**.

And a third. If the hash move isn't singular, but its hash score beats beta (while
`singular_beta` doesn't, or multi-cut would have cut the node off), the node will most likely
fail high whichever good move it searches. A cutoff needs any move that beats beta, not the
best line after the hash move, so that line needs less care: the hash move is searched a ply
*shallower* than usual (`negative_extension_plies`, 1). That is a **negative extension**: the
same test that extends a forced move a ply reduces one with good alternatives.

How far `singular_beta` sits below the hash score is `singular_margin`: 2 centipawns per ply
of depth at first, 3 now. One number moves all three outcomes. A wider margin lowers
`singular_beta`, so it asks more of a singular move and fewer moves are extended. Multi-cut is
pulled both ways: another move reaches the lower `singular_beta` more easily, but the lower
`singular_beta` is at least beta less often. And more hash moves fall between the two, not
singular but beating beta, and are searched a ply shallower. From depth 4,
a margin of 3 makes the bench 18% smaller (1,098,926 nodes against 1,343,312), and over the six
positions of "What it saves" at depth 10 it runs a fifth fewer tests and finds about a quarter
fewer extensions and multi-cuts. A margin of 1 makes the bench 6% smaller too (1,266,996),
from the other side: more extensions, but more multi-cuts as well. Node counts don't say which
plays better, so 3 went to the games: about **+5 Elo** (see "What it's worth").

A margin of 4 makes the bench 8% smaller again (1,006,594), but the six positions at depth 10
grow by 3%: it extends 4% fewer hash moves than 3 and makes 13% more multi-cuts. That is the
first pull winning: from 3 to 4, multi-cuts went up, where from 2 to 3 they went down. In the
games, 4 was no better than 3, about **−2 Elo**, so the margin stays at 3 (see "What it's
worth").

The test's other number is how deep the hash entry has to be. With `singular_tt_depth_margin`
at 3, as in Stockfish (GPL-3.0), an entry counts if it is at most 3 plies shallower than the
node, so at depth 4 an entry from a one-ply search will do. cb2026 stores no entries from the
quiescence search, so every entry has a depth of at least 1, and a margin of 4 changes nothing
at depth 4: from depth 5 on, it lets in entries a ply shallower than before. That makes the
bench 3.5% smaller (1,028,535 nodes against 1,065,754), and the six positions of "What it
saves" at depth 10 9% smaller (574,643 against 634,430). They run 3% more tests and extend 4%
fewer hash moves, but make 11% more multi-cuts and search 19% more hash moves shallower: a score
from a shallower search is a rougher guess, and more of the extra tests end in a cutoff or a
reduction than in an extension. A smaller tree is no promise. A `singular_margin` of 4 made
the bench 8% smaller and failed, and so did capture history, ProbCut and the first version of
correction history's piece tables (chapters 10.2, 12.1 and 14.2). In the games, 4 was worse
than 3, about **−7 Elo**, so the margin stays at 3 (see "What it's worth").

The search answers one more question. Its score is the best any other move managed, so it says
not only whether the hash move is singular but by how much. When no other move comes within
`double_extension_margin` (20 centipawns) of `singular_beta` either, the hash move is close to
the only move: a recapture with nothing else to do, the one defence that doesn't lose a piece.
A mistake in its line costs the most of all, so it is searched two plies deeper instead of one.
That is a **double extension**.

Two plies at a time feed on themselves faster than one, so double extensions have two limits of
their own, on top of the limit of twice the root depth that every extension has. They apply
only off the principal variation, in the null-window searches that make up most of the tree.
And a line can have at most `double_extension_limit` (6) of them: the searcher counts them on
the way down and takes them off on the way up.

They apply more often than the name suggests, because the search's score is usually not just
under `singular_beta` but far below it. Over the six positions of "What it saves" at depth 10,
seven singular hash moves in ten are extended twice, and with a margin of 200 centipawns a third
still would be. With the margin at 20 the depth-10 tree is 44% bigger and the bench 14%
(1,248,666 nodes against 1,098,926). A margin of 50 makes the tree 30% bigger, and 100 only 9%.
At this depth the limit hardly matters: at a margin of 50, a limit of 12 gives the same tree as
6, and only a limit of 3 takes the margin of 20's growth down to 23%. The margin is small, as in
Stockfish.

**Double extensions had no verdict.** An SPRT against the same engine without them found about
+4 Elo, give or take 5, after 8,670 games (see "What it's worth"). Like the first version of
history in LMR (below), that sits between the test's two hypotheses, 0 and 5 Elo, where an SPRT
takes longest, so the test was stopped and double extensions are off by default (`set doubleext
on` turns them on). The candidates for another test are the ones that grow the tree less, a
margin of 50 or a limit of 3, and the long time control, 40+0.4.

The search without the hash move asks a different question from the node's own search, so it
must not mix with it. It neither reads nor writes the transposition table (an entry about the
node with all its moves would answer the wrong question, and its result would poison the
entry), it tries no null move, and IIR doesn't reduce it further. If the hash move is the only
legal move, the search has nothing to search: it fails low, since nothing else comes close.

Singular extensions go back to Deep Thought, the Carnegie Mellon machine that led to Deep Blue
(Anantharaman, Campbell and Hsu, 1988). This version, multi-cut, and negative and double
extensions are as in Stockfish (GPL-3.0); the code is cb2026's own.

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
the same kind of game, after continuation history came in (and before singular extensions):

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

**This version passed**, by about **+22 Elo** (see "What it's worth"). The first changed too few
reductions to show any effect in a reasonable number of games; changing one in seven, in both
directions, and with the same score that orders the moves, made the difference. cb2026 ships
with it on.

## What it saves

The six positions of chapters 10 to 14 (the first six bench positions) at depth 8, measured
while singular extensions started at depth 8, so that none applied:

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
divisor and both directions, makes the tree 2% bigger here. With singular extensions in, it
makes the bench 1.1% bigger, and the six positions at depth 10 3% smaller (674,643 nodes against
694,595): reducing some moves more and others less doesn't add up the same way at every
depth.

Singular extensions don't appear in that table: at depth 8, no node below the root had 8 plies
left. From depth 4 they do, and make the depth-8 tree 57% bigger (232,328 nodes against
148,427; from depth 6, 242,552). At depth 10, starting from depth 8, they made the tree **85% bigger** (694,595 nodes
against 375,334). Over the six positions the search tried 472 hash moves, extended 120 and cut
off 138 nodes by multi-cut;
without multi-cut the tree is 7% bigger again. That is the price of extending near the root:
each extension adds a ply to a large part of the tree. The games say it is worth it: about **+30
Elo** (see "What it's worth"), the biggest gain of anything in this chapter, with a tree almost
twice the size. Nodes aren't Elo, in this direction too.

Negative extensions came later, on top of history in LMR's second version. At depth 10 the
six positions tried 485 hash moves for singularity, extended 125, cut off 142 nodes by
multi-cut and searched 56 hash moves a ply shallower. That takes 0.9% off the tree (668,426
nodes against 674,643) and 2.3% off the bench: a small change, since it applies only where the
singular test has already run and the hash move is expected to cut off anyway. The games agree
that it is small, about **+5 Elo**, and it took over 14,000 of them to say so.

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
- **A negative extension trusts the hash score.** It reduces the move the node expects to cut
  off with, on the strength of a score from an earlier search. If that score was too
  optimistic, the move's shallower search is the one that has to find out. It applies only
  where the singular test has run, with an entry at most `singular_tt_depth_margin` plies
  shallower than the node.
- **Count along the line, and take the count off on every way out.** The limit on double
  extensions is per line, so the searcher keeps a count that each double-extended move adds to
  before its search and takes off after it. A count that some exit skips (a cutoff, a stopped
  search) only grows, until no move can be extended twice. A unit test checks that the count is
  back at zero after a search.
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
| History in LMR, second version (history plus continuation history, divisor 128), vs `main` without it (with singular extensions and adaptive time), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+22.2 ± 11.4 Elo** over 1,661 games (+437 −331 =893): passed (LLR 2.95) |
| Negative extensions vs `main` without them (with adaptive time), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+4.9 ± 4.0 Elo** over 14,211 games (+3518 −3317 =7376): passed (LLR 2.96) |
| Singular extensions from depth 6 vs `main` (from depth 8, with negative extensions), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+5.0 ± 4.1 Elo** over 12,282 games (+2758 −2580 =6944): passed (LLR 2.97) |
| Singular extensions from depth 4 vs `main` (from depth 6), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+6.1 ± 4.8 Elo** over 9,057 games (+2127 −1969 =4961): passed (LLR 2.96) |
| `singular_margin` 3 vs `main` (2, from depth 4), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **+5.2 ± 4.2 Elo** over 11,400 games (+2552 −2381 =6467): passed (LLR 2.96) |
| `singular_margin` 4 vs `main` (3), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **−1.8 ± 5.3 Elo** over 7,476 games (+1670 −1709 =4097): failed (LLR −2.95); the margin stays at 3 |
| `singular_tt_depth_margin` 4 vs `main` (3), 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **−7.4 ± 8.0 Elo** over 3,345 games (+735 −806 =1804): failed (LLR −2.97); the margin stays at 3 |
| Double extensions (margin 20, at most 6 a line) vs `main` without them, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | +3.6 ± 4.9 Elo over 8,670 games (+1988 −1897 =4785): no verdict (LLR 0.92), stopped; off by default |

## Further reading

- Chess Programming Wiki: [Extensions](https://www.chessprogramming.org/Extensions),
  [Check Extensions](https://www.chessprogramming.org/Check_Extensions),
  [Singular Extensions](https://www.chessprogramming.org/Singular_Extensions),
  [Internal Iterative Deepening](https://www.chessprogramming.org/Internal_Iterative_Deepening),
  [Internal Iterative Reductions](https://www.chessprogramming.org/Internal_Iterative_Reductions),
  [Improving](https://www.chessprogramming.org/Improving).
