# Strength levels

*Stage 19 · tag `stage-19-levels` · code in `crates/engine/src/levels.rs` and `lib.rs`*

Everything so far has made cb2026 stronger. This chapter makes it *weaker*, on purpose, for
the many players who want an opponent they can beat sometimes. Done badly, this is easy and
unpleasant to play against. Done well, the engine feels like a human of the chosen strength.

## Why "search less" isn't enough

The obvious knob is depth or nodes: give the engine 500 nodes instead of 5 million. It does
get weaker, but in an odd way. Even a tiny search, with the quiescence search behind it, sees
every hanging piece and every one-move tactic. Its mistakes are the subtle long-term ones a
strong player makes, while a club player's are the tactical oversights the engine never makes.
Its moves look like a strong player's whose judgement has gone strange.

## Two knobs, combined

cb2026's levels combine a smaller search with **choosing among the top moves**:

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:settings}}
```

- **Nodes per move** grow by half with each level: 400 at level 0, about 23,000 at level 10,
  about 890,000 at level 19. Level 20 is full strength, with no limit.
- **Candidates.** The engine searches the best 4 moves (using MultiPV, below), and then picks
  one at random, weighting each by how close it is to the best:

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:choose}}
```

A move scoring the same as the best is as likely as the best. One a few dozen centipawns worse
is still common at low levels. One that throws away a piece is outside the **margin** and is
never played. The weak levels make inaccuracies, and more of them the lower the level,
instead of random blunders.

## MultiPV

To choose among the best moves, the engine has to know the best *several* moves, not just the
best one. **MultiPV** (multiple principal variations) searches the root once per line: line 1
normally, line 2 with line 1's move left out, line 3 with both left out, and so on.

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:main_search}}
```

MultiPV is also useful in its own right: in analysis, GUIs show the top lines side by side
(`setoption name MultiPV value 3`, or `set multipv 3` in the shell). Each line costs nearly a
full search, so three lines take about three times as long.

## Options

| Option | What it does |
| --- | --- |
| `Skill Level` 0–20 | The level; 20 is full strength |
| `UCI_LimitStrength` + `UCI_Elo` 1000–2400 | Limit to an Elo rating; mapped onto a level |
| `MultiPV` 1–256 | Lines searched and reported |

In the shell, `level 5`, `elo 1500` and `elo off` do the same.

```text
> level 5
Strength: level 5: 3037 nodes per move, choosing among the best 4 moves
```

## Calibration

What Elo is level 5? Nobody knows until it plays. The PRD requires the levels to be
**calibrated**: each one plays a few hundred games against reference engines with known ratings
on the CCRL list, and the `UCI_Elo` → level table is fitted to the results. Until then, the
mapping is a straight line from 1000 Elo at level 0 to 2400 at level 19, and the shell calls it
"provisional". Calibration needs the reference engines and a few hours of games, so it runs on
the test Mac together with the rating gauntlets.

The ratings are on the CCRL scale (engine against engine, at blitz). That isn't the same as a
FIDE or Lichess rating, and a human's experience against a level depends on more than its
rating: how it plays, as above.

## Pitfalls

- **Node limits checked too rarely.** The search used to check its limits every 1024 nodes.
  At 400 nodes a move that's far too coarse, so the node count is now checked at every node
  (it costs nothing, unlike reading the clock).
- **Randomness at full strength.** Level 20 must always play the best move, or test results
  would be noise. With `temperature` 0, the choice is the first line.
- **Repeating the same game.** The dice come from the clock and the position, so a weak level
  varies its play from game to game.

## What it's worth

At full strength nothing changes (the bench is unchanged). The levels' strengths are measured
in the calibration run on the test Mac.

## Further reading

- Chess Programming Wiki: [Strength](https://www.chessprogramming.org/Strength),
  [Multi-PV](https://www.chessprogramming.org/Principal_Variation#Multi-PV).
