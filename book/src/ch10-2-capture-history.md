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
after stage 20, so no stage tag contains it. Switch it with `set capthist on|off`.

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

```rust,ignore
{{#include ../../crates/engine/src/ordering.rs:noisy_score}}
```

MVV-LVA's key rises by 8 for each rank of victim, from pawn to queen, and falls by 1 for each
rank of attacker. Scaled by 1,024, a victim rank is worth 8,192, half of the 16,384 that a
history score can reach, and an attacker rank is worth 1,024. So:

- **Among captures of the same piece, capture history decides** once it disagrees with MVV-LVA
  by more than the attackers' difference: 3,072 between a pawn and a rook.
- **Across victims it moves a capture two ranks at most.** A rook's capture of a pawn with the
  best history there is gets as far as a rook's capture of a bishop with none, and no further.
  The biggest captures keep their place near the front.
- **The static exchange split stays.** Captures that lose material by SEE still come after the
  quiet moves, in this same order among themselves.

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
does that: it scores a capture by its victim's value and its capture history. cb2026 keeps
the attacker term as the starting order, so at a node the table knows nothing about, the
captures come in chapter 7's order. The code here is cb2026's own.

## What it changes

The bench is 11.5% smaller with it: 943,210 nodes instead of 1,065,754. With `set capthist off`
it is `main`'s exactly, so the change does nothing when switched off. The speed is unchanged:
three alternating bench runs gave 1.51 to 1.65 million nodes a second for `main` and 1.61 to
1.63 million with capture history. The table is small enough to stay in the cache.

Chapter 10.1 warned that with late move reductions and pruning, a smaller tree doesn't prove
better ordering: a worse order can push good moves into the reductions and shrink the tree by
missing them. That trap is mostly absent here. Captures are never reduced or pruned for coming
late, so a capture moved later is still searched in full. The 11.5% is probably cutoffs found
sooner. Whether it is worth Elo, only games can say.

## Pitfalls

- **Swamping MVV-LVA.** With too large a history score, a capture of a pawn that happened to
  cut off a few times would be searched before a free queen. The scale keeps history to two
  victim ranks either way.
- **Bad captures.** History doesn't move a capture across the static exchange split, so a
  losing capture with a good history still waits behind the quiet moves.
- **Under-promotions** are quiet moves to the ordering (chapter 7), so their history is in the
  quiet tables, not here.

## What it's worth

| Test | Result |
| --- | --- |
| Capture history vs `main` without it, 8+0.08, test Mac (M3 Max), SPRT [0, 5] | Waiting for its SPRT (cjus/chessengine#115) |

## Further reading

- Chess Programming Wiki:
  [History Heuristic](https://www.chessprogramming.org/History_Heuristic#Capture_History),
  section "Capture History".
- Stockfish's source (GPL-3.0), `movepick.cpp` and `search.cpp`: how captures are scored, and
  the capture history updates.
