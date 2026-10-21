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
| `UCI_LimitStrength` + `UCI_Elo` 650–2450 | Limit to an Elo rating; mapped onto the levels' measured ratings (below) |
| `MultiPV` 1–256 | Lines searched and reported |

In the shell, `level 5`, `elo 1300` and `elo off` do the same, and say what the engine now
plays at:

```text
> level 5
Strength: level 5, about 957 Elo: 3037 nodes per move, choosing among the best 4 moves
> level 20
Strength: full, about 2782 Elo
> elo 1300
Strength: level 9.1, about 1300 Elo: 15705 nodes per move, choosing among the best 4 moves
```

`Skill Level` caps `UCI_Elo`: with `level 5` set, `elo 1300` still plays at level 5.

## Calibration

What Elo is level 5? Nobody knows until it plays. The PRD requires the levels to be
**calibrated**: each one plays games against reference engines with known ratings on the CCRL
list, and `UCI_Elo` is fitted to the results.

`scripts/mac-levels.sh` does that on the test Mac. Each level plays 200 games against each of
the four reference engines nearest its expected strength, chosen from twelve open-source
engines rated 1058 to 3049 on CCRL Blitz (the PRD lists them). The gauntlet tool fits the
level's rating on their scale, holding theirs fixed.

It ran twice. The first run, on 2026-10-18 (cjus/chessengine#103), rated every level from 0
to 19 at 8+0.08: 16,000 games in about six and a half hours. A check of levels 4, 10 and 16 at
40+0.4 then rated them 111 to 272 lower (cjus/chessengine#106; "Pitfalls" says why). So the
second run rated every other level at 40+0.4: levels 0, 2, 6, 8, 12, 14, 18 and 19, with 4, 10
and 16 from the check, 8,800 games in about fourteen and a half hours. Each odd level below 19
is put halfway between its neighbours. In the first run's table, that lands within 25 of the
odd levels' measured ratings, less than each level's own margin.

| Level | Nodes per move | Rating at 40+0.4 | At 8+0.08 |
| ---: | ---: | --- | --- |
| 0 | 400 | 629 ± 55 | 752 ± 41 |
| 1 | 600 | *703* | 823 ± 35 |
| 2 | 900 | 777 ± 36 | 851 ± 34 |
| 3 | 1,350 | *833* | 927 ± 30 |
| 4 | 2,025 | 888 ± 32 | 999 ± 30 |
| 5 | 3,037 | *957* | 1052 ± 27 |
| 6 | 4,556 | 1025 ± 27 | 1131 ± 25 |
| 7 | 6,834 | *1112* | 1242 ± 24 |
| 8 | 10,251 | 1198 ± 24 | 1307 ± 24 |
| 9 | 15,377 | *1295* | 1414 ± 24 |
| 10 | 23,066 | 1391 ± 25 | 1534 ± 25 |
| 11 | 34,599 | *1501* | 1642 ± 26 |
| 12 | 51,898 | 1610 ± 26 | 1783 ± 28 |
| 13 | 77,847 | *1731* | 1924 ± 30 |
| 14 | 116,771 | 1852 ± 26 | 2106 ± 27 |
| 15 | 175,157 | *1974* | 2240 ± 26 |
| 16 | 262,736 | 2096 ± 27 | 2368 ± 24 |
| 17 | 394,104 | *2217* | 2475 ± 24 |
| 18 | 591,156 | 2337 ± 25 | 2533 ± 25 |
| 19 | 886,735 | 2483 ± 23 | 2599 ± 22 |

The ratings in italics are halfway between their neighbours, not measured.

Full strength rated 2782 ± 11 in the gauntlet of 2026-10-21, at 8+0.08 (2714 ± 10 on 2026-10-18,
before correction history and double extensions). It uses the clock, so unlike the levels it
gains from more time much as its opponents do. Level 19 plays about 300 below it. The 40+0.4 column goes into the code:

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:ratings}}
```

Before any calibration, the levels followed a guessed straight line from 1000 at level 0 to
2400 at level 19. That was about 370 too high at the bottom and close at the top. A level gains
55 to 75 points up to level 6, where the randomness dominates, then more with each level: about
120 from level 12 up, and 146 from 18 to 19.

`UCI_Elo` maps onto the table. Between two levels, the strength is in between too: the node
count, margin and temperature are smooth in the level, so `settings_at` takes a fractional one,
and an Elo of 1300, between level 9 (1295) and level 10 (1391), plays at level 9.1:

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:level_for_elo}}
```

The `UCI_Elo` range was 1000 to 2400 before any calibration, a guess at what the levels would
reach, and 800 to 2600 after the first. It now runs from level 0's rating to level 19's,
rounded inward to 50: **650 to 2450**, levels 0.3 to 18.8. `Skill Level 0` and `Skill Level
19` still play their own strengths.

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
- **A level doesn't get stronger with more time.** Its node limit is the same at any time
  control, while its opponents use the time they're given. The first calibration ran at
  8+0.08, and the check at 40+0.4 rated levels 4, 10 and 16 lower by 111, 143 and 272. So the
  table now comes from 40+0.4. That is still faster than CCRL's own 2'+1", where the levels
  would rate a little lower again. The top levels lost less than the middle ones (116 at level
  19, 196 at 18), probably because the search also stops when the clock runs out. At 8+0.08
  their node counts didn't always fit in the time, so they played weaker than their limits
  there; at 40+0.4 they search them in full.
- **Level 0 is rated from above.** It is weaker than every reference engine: at 40+0.4 it
  scored 6% against the weakest (1058) and almost nothing against the rest, so its rating has
  the widest margin (± 55). No engine below 1000 passed the checks for the reference list.
- **The reference engines have bugs too.** Walleye 1.3.0 ignores an en passant capture in the
  moves it is sent, so its board goes wrong and it later plays an illegal move: about 1% of its
  games, which it loses. CCRL's games saw the same bug, so its rating includes it. Arche 0.3.8
  twice answered `bestmove 0000` (no move) in a position with legal moves, at 40+0.4 only,
  and lost those games too.

## What it's worth

At full strength nothing changes (the bench is unchanged). The levels span 629 to 2483 on the
CCRL Blitz scale at 40+0.4, from a beginner's engine to a strong one, and `UCI_Elo` now means
what it says, within each level's margin of about 25 points (more at the bottom).

## Further reading

- Chess Programming Wiki: [Strength](https://www.chessprogramming.org/Strength),
  [Multi-PV](https://www.chessprogramming.org/Principal_Variation#Multi-PV).
