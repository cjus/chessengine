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
| `UCI_LimitStrength` + `UCI_Elo` 800–2600 | Limit to an Elo rating; mapped onto the levels' measured ratings (below) |
| `MultiPV` 1–256 | Lines searched and reported |

In the shell, `level 5`, `elo 1500` and `elo off` do the same, and say what the engine now
plays at:

```text
> level 5
Strength: level 5, about 1052 Elo: 3037 nodes per move, choosing among the best 4 moves
> level 20
Strength: full, about 2714 Elo
> elo 1500
Strength: level 9.7, about 1500 Elo: 20562 nodes per move, choosing among the best 4 moves
```

`Skill Level` caps `UCI_Elo`: with `level 5` set, `elo 1500` still plays at level 5.

## Calibration

What Elo is level 5? Nobody knows until it plays. The PRD requires the levels to be
**calibrated**: each one plays games against reference engines with known ratings on the CCRL
list, and `UCI_Elo` is fitted to the results.

`scripts/mac-levels.sh` did that on the test Mac on 2026-10-18 (cjus/chessengine#103). Each
level from 0 to 19 played 200 games at 8+0.08 against each of the four reference engines
nearest its expected strength, chosen from twelve open-source engines rated 1058 to 3049 on CCRL
Blitz (the PRD lists them). The gauntlet tool fitted each level's rating on their scale, holding
theirs fixed. That was 16,000 games, in about six and a half hours:

| Level | Nodes per move | Rating | | Level | Nodes per move | Rating |
| ---: | ---: | --- | --- | ---: | ---: | --- |
| 0 | 400 | 752 ± 41 | | 10 | 23,066 | 1534 ± 25 |
| 1 | 600 | 823 ± 35 | | 11 | 34,599 | 1642 ± 26 |
| 2 | 900 | 851 ± 34 | | 12 | 51,898 | 1783 ± 28 |
| 3 | 1,350 | 927 ± 30 | | 13 | 77,847 | 1924 ± 30 |
| 4 | 2,025 | 999 ± 30 | | 14 | 116,771 | 2106 ± 27 |
| 5 | 3,037 | 1052 ± 27 | | 15 | 175,157 | 2240 ± 26 |
| 6 | 4,556 | 1131 ± 25 | | 16 | 262,736 | 2368 ± 24 |
| 7 | 6,834 | 1242 ± 24 | | 17 | 394,104 | 2475 ± 24 |
| 8 | 10,251 | 1307 ± 24 | | 18 | 591,156 | 2533 ± 25 |
| 9 | 15,377 | 1414 ± 24 | | 19 | 886,735 | 2599 ± 22 |

Full strength rated 2714 ± 10 in the gauntlet of the same day, so level 19 plays about 115
below it. The table goes into the code as it was measured:

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:ratings}}
```

The guess it replaces, a straight line from 1000 at level 0 to 2400 at level 19, was too flat:
the bottom levels are about 250 weaker than it said, and the top ones about 200 stronger. A
level gains about 60 points at the bottom, where the randomness dominates, 110 to 180 in the
middle, and 60 again at 18 and 19, which already find most of what full strength finds.

`UCI_Elo` maps onto the table. Between two levels, the strength is in between too: the node
count, margin and temperature are smooth in the level, so `settings_at` takes a fractional one,
and an Elo of 1500, between level 9 (1414) and level 10 (1534), plays at level 9.7:

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:level_for_elo}}
```

The `UCI_Elo` range was 1000 to 2400 before the calibration, a guess at what the levels would
reach. They reach further, so it is now **800 to 2600**: levels 0.7 to 19.0. It stops short of
level 0 (752), whose rating is the least certain; `Skill Level 0` still plays it.

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
- **A level doesn't get stronger with more time.** It limits nodes, not time, so it plays the
  same at any time control, while its opponents use theirs. The calibration ran at 8+0.08, far
  faster than CCRL's 2'+1"; at CCRL's own time control the reference engines would play
  stronger and the levels would rate lower. By how much needs a run at a slower time control.
- **Level 0 is rated from above.** It is weaker than every reference engine: it scored 12.5%
  against the weakest (1058) and almost nothing against the rest, so its rating has the widest
  margin. No engine below 1000 passed the checks for the reference list.
- **The reference engines have bugs too.** Walleye 1.3.0 ignores an en passant capture in the
  moves it is sent, so its board goes wrong and it later plays an illegal move: about 1% of its
  games, which it loses. CCRL's games saw the same bug, so its rating includes it.

## What it's worth

At full strength nothing changes (the bench is unchanged). The levels span 752 to 2599 on the
CCRL Blitz scale at 8+0.08, from a beginner's engine to a strong one, and `UCI_Elo` now means
what it says, within each level's margin of about 25 points.

## Further reading

- Chess Programming Wiki: [Strength](https://www.chessprogramming.org/Strength),
  [Multi-PV](https://www.chessprogramming.org/Principal_Variation#Multi-PV).
