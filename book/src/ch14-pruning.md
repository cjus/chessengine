# Futility, razoring and SEE pruning

*Stage 14 · tag `stage-14-pruning` · code in `crates/engine/src/search.rs` and `see.rs`*

Chapters 12 and 13 cut whole subtrees (null move) and searched late moves less deeply
(LMR). This chapter adds the last family of selective techniques in cb2026's hand-crafted
search: pruning near the leaves, decided by how far the static evaluation is from the
window. Each one is a small, cheap guess. Together they make the tree nearly five times
smaller.

They all share the same ground rules. None of them applies on the principal variation, in
check, or (for the move-level ones) before a move has been found that doesn't lose to a mate.

## Static exchange evaluation

Several of these techniques need to know whether a move loses material, so first a tool:
**static exchange evaluation** (SEE). It plays out every capture and recapture on one square,
each side using its least valuable piece first and free to stop when continuing would lose
more. It runs without any search, so it is cheap enough to call on every capture.

```rust,ignore
{{#include ../../crates/engine/src/see.rs:see}}
```

The `gains` list records, after each capture, what the capturing side would have gained if the
exchange stopped there. Walking it backwards lets each side choose between stopping and going
on. When a piece leaves the square's line, the attackers are recomputed with it removed from
the occupancy, which reveals **x-ray** attackers: the rook behind a rook, the bishop behind a
queen.

SEE improves ordering too. MVV-LVA put QxP before every quiet move even when the pawn is
defended. Now captures that lose material are searched last, after the quiet moves.

## Reverse futility pruning

If the static evaluation is above beta by a margin that grows with depth (80 centipawns per
ply, up to depth 7), the side to move is probably doing so well that the opponent can't bring
the score back down within the remaining depth. The node returns the static evaluation
straight away. This is also called the **static null move**: it is null-move pruning (chapter
12) without the search.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:reverse_futility}}
```

## Razoring

The mirror image: at depth 1 or 2, if the static evaluation is far *below* alpha (300
centipawns per ply), quiet moves are unlikely to rescue the position. Only a capture could, so
the node asks the quiescence search. If even the captures can't reach alpha, the node is
abandoned.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:razoring}}
```

## Futility pruning

The same idea per move. Within 6 plies of the horizon, if the static evaluation plus a
generous margin (100 + 100 per ply) still doesn't reach alpha, a quiet move is "futile". It
changes no material, so it can hardly gain that much, and it is skipped without being searched.
Captures and checks are still searched.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:futility}}
```

## SEE pruning

Within 6 plies of the horizon, moves that lose material by SEE are skipped: quiet moves that
put a piece where it can be taken for more than 60 centipawns per ply of depth, and captures
that lose more than 100 per ply. In the quiescence search every losing capture is skipped:
captures there are meant to resolve the position, and a capture that loses material can't
improve on standing pat.

```rust,ignore
{{#include ../../crates/engine/src/search.rs:see_pruning}}
```

## Delta pruning

In the quiescence search, a capture is skipped if even winning the captured piece for free,
plus a 200-centipawn safety margin, would leave the score at or below alpha. A side that is
a rook down won't be saved by winning a pawn.

## What it saves

The six positions at depth 8, as in the earlier chapters:

| Setting | Nodes | vs all on |
| --- | ---: | ---: |
| All on | 300,790 | — |
| `set rfp off` | 323,534 | +8% |
| `set razoring off` | 306,230 | +2% |
| `set futility off` | 314,032 | +4% |
| `set seeprune off` | 583,815 | +94% |
| `set delta off` | 343,890 | +14% |
| `set seeorder off` | 266,118 | −12% |
| All six off | 1,389,854 | +362% |

SEE pruning, mostly in the quiescence search, does the most. Ordering losing captures last
*costs* nodes on these six positions. Moving a losing capture behind the quiet moves isn't
always right: some of those captures are sacrifices that work. Whether it's worth keeping is
for the SPRT to say, not a node count.

The bench now searches to depth 10 instead of 4: at depth 4 the pruning left too little of the
tree to be a useful fingerprint.

## Pitfalls

- **Margins.** Every technique here trades accuracy for depth through a margin. Too small,
  and the engine prunes good moves; too large, and it prunes nothing. The values here are
  common starting points, and stage 16 and later tuning will revisit them.
- **Mates.** Pruning by static evaluation is blind to mating attacks. That's why none of it
  applies in check, and the move-level pruning waits until a move has been found that
  doesn't lose to a mate.
- **SEE's blind spots.** SEE ignores pins and checks, and it only looks at one square. A
  capture it calls losing can win if the recapturing piece is pinned.
- **Interactions.** These techniques overlap with each other and with null move and LMR:
  turning one off shows only what it adds *on top of* the others.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 14 vs stage 13, 8+0.08, SPRT [0, 5] | SPRT_RESULT |

## Further reading

- Chess Programming Wiki: [Static Exchange Evaluation](https://www.chessprogramming.org/Static_Exchange_Evaluation),
  [Reverse Futility Pruning](https://www.chessprogramming.org/Reverse_Futility_Pruning),
  [Razoring](https://www.chessprogramming.org/Razoring),
  [Futility Pruning](https://www.chessprogramming.org/Futility_Pruning),
  [Delta Pruning](https://www.chessprogramming.org/Delta_Pruning).
- Ernst Heinz, "Extended Futility Pruning", *ICCA Journal* 21 (1998).
