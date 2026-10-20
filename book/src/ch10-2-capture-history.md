# Capture history

*Milestone M5, after stage 20 · on `main`, no stage tag · code in
`crates/engine/src/ordering.rs` and `crates/engine/src/search.rs`*

Chapter 7 orders captures by MVV-LVA: the biggest victim first and, among captures of the same
piece, the smallest attacker first. It is a good rule, and a fixed one. Bxf7 comes before Rxf7
in every position, whether or not the bishop's capture works there, and a capture of a pawn
always waits behind every capture of a knight, even where taking the pawn is what refutes the
opponent's last move.

Chapter 10 had the same problem with quiet moves and solved it with the history table, which
learns from the search which moves cause cutoffs. **Capture history** does the same for
captures. Its table is indexed by the piece that captures, the square it captures on and the
piece it takes, and it learns the way history does. Like chapter 14.1's techniques, it came
after stage 20, so no stage tag contains it. Its first version measured weaker. The second, in
Stockfish's form, is on for its test (cjus/chessengine#129); `set capthist off` switches it
off.

## The table

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:capture_index}}
```

Twelve pieces, 64 squares and six victims make 4,608 scores, 18 KB. The victim keeps apart
moves that mean different things: a rook arriving on d5 to take a pawn and one arriving to
take a knight. The sixth victim is nothing, for a queen promotion that captures nothing, which
the ordering treats as a capture (chapter 7). A king is never captured, so its slot is free for
it. An en passant capture takes a pawn that isn't on the destination square, so its victim is
set by hand.

## Ordering

A capture starts from what it wins, by the piece values of static exchange evaluation (chapter
14): the victim's value, and a queen less a pawn for a queen promotion.

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:capture_value}}
```

Its ordering key is that value times 32, plus its capture history score:

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:noisy_score}}
```

Times 32, a pawn is worth 3,200, a fifth of the 16,384 that a history score can reach; a knight
10,240, a rook 16,000 and a queen 28,800. So:

- **Among captures of the same piece, capture history decides.** The attacker is left out, as
  in Stockfish, except to break ties: its rank, 1 to 6, is less than one cutoff's bonus from
  depth 3 on. At a node the table knows nothing about, every capture of the same piece ties,
  and the smaller attacker goes first, as in MVV-LVA.
- **Across victims it can move a capture a long way, but not all the way.** A capture of a pawn
  with the best history there is (3,200 + 16,384) comes before a capture of a rook with none
  (16,000), but never before a capture of a queen. A knight and a bishop are only 320 points
  apart (10 centipawns), so between them history decides soon.
- **The static exchange split stays.** Captures that lose material by SEE still come after the
  quiet moves, in this same order among themselves.

Stockfish 16 (GPL-3.0) adds 7 times the victim's middlegame value to a capture history limited
to 10,692. That puts a queen at 1.66 times the limit, where cb2026's scale puts it at 1.76. Its
pawn is worth less (126 against a knight's 781, where SEE's values give 100 and 320), so a
pawn's capture moves further on its history there.

### The first version

The first version kept MVV-LVA's attacker term. Its key was the MVV-LVA key times 1,024 plus
capture history. MVV-LVA's key rises by 8 for each rank of victim and falls by 1 for each rank
of attacker, so a victim rank was worth 8,192, half of what a history score can reach, and an
attacker rank 1,024. Among captures of the same piece, history decided only once it disagreed
with MVV-LVA by more than the attackers' difference (3,072 between a pawn and a rook), and
across victims it moved a capture two ranks at most. It took 11.5% off the bench and lost
about 19 Elo (see "What it's worth").

The quiescence search orders its captures with the same table. It doesn't update it: its
nodes have no depth left, and the bonus is `depth²`.

## Learning

When a move causes a beta cutoff, the captures searched before it at that node get a penalty,
and the move gets a bonus if it is a capture:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:cutoff}}
```

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:capture_update}}
```

The penalty goes to the captures tried before a *quiet* move's cutoff too. Every capture that
doesn't lose material is searched before the killers and the other quiet moves, so when a
quiet move cuts off, those captures all failed, and that is worth knowing. Without the
penalties, scores could only rise. The bonus is chapter 10's `depth²`, with the same gravity,
and the table is halved between searches like the others.

## Credit

The Chess Programming Wiki credits capture history to Stefan Geschwentner (2016) and
describes it as replacing MVV-LVA's attacker term with the history score. Stockfish (GPL-3.0)
does that: it scores a capture by its victim's value and its capture history. The second
version follows it. The first kept the attacker term as a large part of the key; the second
keeps it only to break ties. Stockfish generates pawn captures first, then the knights',
bishops', rooks' and queens', so its ties tend to go to the smaller attacker too. The code here
is cb2026's own.

## What it changes

The first version made the bench 11.5% smaller: 943,210 nodes instead of 1,065,754. The second
makes it 12.1% smaller, 998,700 instead of 1,136,500 (`main` by then had double extensions,
chapter 14.1). With `set capthist off` it is `main`'s exactly, so the change does nothing when
switched off. The speed is unchanged: over four alternating pairs of bench runs, the second
version's speed was 0.996 to 1.004 times `main`'s. The table is small enough to stay in the
cache.

The bench is one depth. Over the first six bench positions, the second version's trees:

| Depth | `set capthist off` | Second version | Cutoffs by the first move (off, on) |
| --- | ---: | ---: | --- |
| 9 | 447,273 | 347,024 (−22%) | 89.8%, 90.1% |
| 10 | 703,714 | 570,471 (−19%) | 89.7%, 89.7% |
| 11 | 885,651 | 1,110,833 (+25%) | 89.7%, 90.0% |

So the trees shrink at one depth and grow at the next, and the share of cutoffs made by the
first move searched hardly moves. Nothing here says whether the order is better.

Chapter 10.1 warned that with late move reductions and pruning, a smaller tree doesn't prove
better ordering: a worse order can push good moves into the reductions and shrink the tree by
missing them. The warning seemed to apply less here. Captures are never reduced or pruned for
coming late, so a capture moved later is still searched in full, and the 11.5% looked like
cutoffs found sooner. The games said otherwise (below): the tree shrank and the engine got
weaker. Why isn't measured. One candidate: at a principal variation node, the order decides
which capture gets the first move's full window, while the others get null windows, where
null-move pruning and the rest are allowed and a good capture can be missed.

## Pitfalls

- **Swamping the victim's value.** With too large a history score, a capture of a pawn that
  happened to cut off a few times would be searched before a free queen. The scale keeps a
  queen out of history's reach, and lets it move a pawn's capture past a rook's.
- **Ties are most of the table at first.** Without the tie-break, the second version made the
  bench 6.5% bigger (1,210,252 nodes), not 12.1% smaller: at a node the table knows nothing
  about, captures of the same piece tied, and the order fell back to the order the moves were
  generated in, rooks before pawns. A rule that only breaks ties can still change the tree a
  lot.
- **Bad captures.** History doesn't move a capture across the static exchange split, so a
  losing capture with a good history still waits behind the quiet moves.
- **Under-promotions** are quiet moves to the ordering (chapter 7), so their history is in the
  quiet tables, not here.

## What it's worth

| Test | Result |
| --- | --- |
| Capture history vs `main` without it, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | **−19.2 ± 11.9 Elo** over 1,561 games (+328 −414 =819): failed (LLR −2.96), cjus/chessengine#115, so it is off by default |

Like switching history off in chapter 10.1, the first version made the tree smaller and the
engine weaker. The switch and the table stayed for a second version, in Stockfish's form: the
victim's value plus capture history, with the attacker only breaking ties. It is being tested
(cjus/chessengine#129). Still untested:

- **History only among captures of the same piece**: a scale large enough that a victim rank
  outweighs any history score, so history only reorders the attackers.
- **No penalty for the captures tried before a quiet move's cutoff**, so that a capture is
  marked down only when another capture did better.

## Further reading

- Chess Programming Wiki:
  [History Heuristic](https://www.chessprogramming.org/History_Heuristic#Capture_History),
  section "Capture History".
- Stockfish's source (GPL-3.0), `movepick.cpp` and `search.cpp`: how captures are scored, and
  the capture history updates.
