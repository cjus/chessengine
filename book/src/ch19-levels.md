# Strength levels

*Stage 19 · tag `stage-19-levels` · code in `crates/engine/src/levels.rs` and `lib.rs`. A
person's horizon and the human scale came later (cjus/chessengine#151, for cjus/chessgui#113),
with the `mistakes` tool in `crates/tools/src/mistakes.rs`.*

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

That turned out to be too tidy. People don't only make inaccuracies: they blunder, often
(below). Every candidate above comes from a search that sees the short tactics, so a level
seldom left a piece where it could be taken, and seldom missed one left for it. Carlos played
the level then rated 1050 and found it "much stronger than a 1050 level player"
(cjus/chessgui#113). Counted the way Lichess counts, it blundered 0.7 times in 40 moves;
people rated about 1000 on Lichess blunder 3.4 times.

## A person's horizon

People's worst mistakes come from how far they look. A beginner moves a piece without seeing
the reply, and it is taken. A stronger player sees the reply but not the move after it, and
walks into a fork or a pin. So a weaker level chooses many of its moves **with a person's
horizon**: a search of exactly one ply below level 8, the move alone, or two from level 8, the
move and the reply, with no quiescence search, no transposition table and nothing pruned
(`Techniques::horizon`). The positions at the end are evaluated as they stand, in the middle of
any exchange, as a hurried eye sees them. Then the level chooses among that search's best moves
as usual.

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:horizon}}
```

Such a move is often fine: it is a blunder only when there was a tactic to miss, which is when
people blunder too. How often a level looks that way is fitted to people's blunders, level by
level (below):

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:horizon_chances}}
```

### Into the search

The dice are rolled first, beside the level's node limit. A move with a person's horizon gets a
depth limit of its one or two plies, and never in analysis: an infinite search is someone
asking what the position holds, not a level playing a game.

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:level_limits}}
```

The dice are the clock's nanoseconds mixed with the position's hash, the same as for the choice
among the best moves, so a level goes its own way in each game:

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:dice}}
```

Then the search runs with another set of techniques, on one thread:

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:horizon_search}}
```

`Techniques::horizon` starts from `Techniques::sound`, the set that only makes the search faster
and never changes its result: alpha-beta, move ordering and PVS, with null move, reductions,
extensions and every kind of pruning off. So no move is skipped and none is searched deeper
for being interesting. It takes away three more, the first of which would look past the
horizon:

```rust,ignore
{{#include ../../crates/engine/src/search.rs:horizon_techniques}}
```

- **The quiescence search** (chapter 7), which would otherwise play out the captures at the
  horizon. Without it, the position after a queen takes a defended pawn is evaluated a pawn up,
  whatever recaptures: that is what a hurried eye sees, and the whole point.
- **Aspiration windows** (chapter 11), which start each iteration around the last one's score.
  At one or two plies there is no last score worth trusting.
- **The transposition table** (chapter 8). A horizon search's scores are unresolved and
  shallow; written to the table, they would be read as real results by the full searches of
  the moves after it. So it neither reads nor writes the table.

One thread, because Lazy SMP's helpers (chapter 17) share their work only through the
transposition table, which this search doesn't use: they would search for nothing. Everything
after the search is as before: it returns its best lines, and `choose` picks one with the
level's margin and temperature.

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
| `UCI_LimitStrength` + `UCI_Elo` 600–2500 | Limit to a rating on the levels' human scale (below) |
| `MultiPV` 1–256 | Lines searched and reported |

In the shell, `level 5`, `elo 1300` and `elo off` do the same, and say what the engine now
plays at:

```text
> level 5
Strength: level 5, about 1100 on Lichess (rapid, by blunders): 3037 nodes per move, choosing among the best 4 moves; a person's horizon (1 ply) on 55% of its moves
> level 20
Strength: full
> elo 1350
Strength: level 7.5, about 1350 on Lichess (rapid, by blunders): 8370 nodes per move, choosing among the best 4 moves; a person's horizon (1 ply) on 46% of its moves
```

`Skill Level` caps `UCI_Elo`: with `level 5` set, `elo 1350` still plays at level 5.

## A human scale

What rating is level 5? Until cjus/chessgui#113 the answer was a rating against other engines,
measured in games (the next section). But a rating against engines says little about what a
person meets: engines punish every blunder at once, people often don't, so a level that
blunders like a person rated 1000 rates far lower against engines, and a level rated 1000
against engines plays far more cleanly than a person rated 1000. Carlos chose a human scale.

**People's blunders.** Lichess publishes every rated game it hosts (<https://database.lichess.org/>),
and about one in twelve carries the evaluation its analysis gave each move. Lichess judges a
move by how much it lowers the mover's winning chances, from −1 to 1: a drop of 0.1 is an
inaccuracy, 0.2 a mistake, 0.3 a blunder, with the chances from centipawns through a curve
fitted to its games (<https://lichess.org/page/accuracy>). The `mistakes` tool counts the same
way (`cb2026_tools::mistakes`). Its `--people` mode, on 60,000 rated rapid games of January
2025 with evaluations, gave, per 40 moves of one player:

| Lichess rating | 600 | 800 | 1000 | 1200 | 1400 | 1600 | 1800 | 2000 | 2200 | 2400 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Blunders | 4.6 | 3.9 | 3.4 | 3.0 | 2.8 | 2.4 | 2.1 | 1.9 | 1.5 | 0.9 |
| Mistakes | 2.0 | 1.8 | 1.7 | 1.7 | 1.7 | 1.6 | 1.5 | 1.3 | 1.2 | 0.9 |
| Inaccuracies | 4.2 | 3.9 | 3.9 | 3.8 | 3.9 | 3.6 | 3.7 | 3.4 | 3.0 | 2.5 |

People blunder two to four times a game across nearly the whole range, and the rate falls
slowly with rating. The levels did far less: 2.8 at level 0, 0.7 at level 6, 0.2 at level 12
and none by level 18, judged by a full-strength search of 100,000 nodes in 32 games each.

### Counting as Lichess counts

Every count above, people's and the levels', comes from the same two functions. An evaluation
becomes the mover's winning chances through Lichess's curve, with centipawns capped at ±1000
and a mate in n counted as (21 − min(10, n)) × 100 centipawns, so a mate soon is a little
surer than a mate later. A move is judged by how far it lowers them:

```rust,ignore
{{#include ../../crates/tools/src/mistakes.rs:chances}}
```

The curve's slope, the cap and the thresholds are Lichess's (its accuracy page and lila's
source, AGPL-3.0: the numbers only, none of its code). The curve is steep near equality and
flat in decided positions, so throwing away a pawn in a level game counts, and throwing away a
rook in a game already lost barely does, as on Lichess.

**People** are counted in Lichess's own games. Each game's moves are one line of the PGN file,
with the evaluation Lichess's analysis gave after each move in a comment, `{ [%eval 0.17] }`
in pawns from White's side, or `[%eval #-3]` for a mate. Each evaluation is White's chances
after one move and before the next; White's fall when White errs, and Black's (the same,
negated) when Black does. A move is counted for its player's rating band:

```rust,ignore
{{#include ../../crates/tools/src/mistakes.rs:judge_people}}
```

**A level** plays itself from the same openings and random first moves as the self-play tools,
and a full-strength search of a fixed number of nodes judges every move. Each position is
searched once: its score is the "after" of one move and, from the other side, the "before" of
the next. A game's end is scored as it stands, a mate as lost, a draw as even:

```rust,ignore
{{#include ../../crates/tools/src/mistakes.rs:judge_level}}
```

The judge is weaker than Lichess's analysis: it misses a blunder now and then, and now and then
calls a sound move one (full strength, which makes none, measured 0.3 in 40 moves). It errs
alike for every level, which is what matters for fitting them to each other; beside people's
counts, from Lichess's own analysis, the levels' are close rather than exact.

**The levels on that scale.** Level 0 is now a Lichess rapid rating of 600 and each level 100
more, to 2500 at level 19, and each level's chance of a person's horizon is fitted so that it
blunders as often as people of its rating:

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:ratings}}
```

The fit took three rounds of 40 games a level with the `mistakes` tool. A level blunders about
as often as its usual search does, plus its chance of a person's horizon times what such a move
adds: about 5 blunders in 40 moves with one ply, 3 to 4 with two. The first round put that at
4.1 for two plies, measured with every move a horizon move; played among a level's other moves,
a two-ply horizon move meets fewer tactics, and the upper levels fell short. Each round set
each level's chance from how far the last missed, and the last is smoothed into one straight
line through levels 0 to 7 and another through 8 to 19, since 40 games of one level carry about
± 0.5 blunders of noise.

**Level 6, worked through.** With its usual search, level 6 blundered 0.7 times in 40 moves. A
one-ply horizon move, played among its other moves, blunders about 5 times in 40. People rated
1200 blunder 3.0 times. So level 6 needs the horizon on about (3.0 − 0.7) / (5 − 0.7) ≈ 0.53 of
its moves. The rounds measured 3.3 blunders at a chance of 0.48 and 2.7 at 0.50, both within
the noise of 3.0, and the straight line through levels 0 to 7 gives it 0.50. From level 8 the
same sum uses a two-ply horizon move's blunders, about 3 in 40 instead of 5, so the chance
jumps up there: a horizon that sees the reply errs less often, and must be used more often to
blunder as much.

The check, with openings none of the rounds used:

| Level | 0 | 2 | 4 | 6 | 8 | 10 | 12 | 14 | 16 | 18 | 19 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Rating | 600 | 800 | 1000 | 1200 | 1400 | 1600 | 1800 | 2000 | 2200 | 2400 | 2500 |
| People's blunders | 4.5 | 4.1 | 3.4 | 3.0 | 2.7 | 2.4 | 2.1 | 1.8 | 1.5 | 1.1 | 0.9 |
| The level's | 3.8 | 3.9 | 2.5 | 3.7 | 2.4 | 2.8 | 2.3 | 1.5 | 1.7 | 1.3 | 1.0 |

Over all twenty levels the level blunders 2.5 times in 40 moves on average, people of the same
ratings 2.6. Full strength measured 0.3, which is about the judge's own floor: a search of
100,000 nodes now and then disagrees with a stronger one.

`UCI_Elo` maps onto the table. Between two levels the strength is in between too: the node
count, margin and temperature are smooth in the level, so `settings_at` takes a fractional one,
and an Elo of 1350, between level 7 (1300) and level 8 (1400), plays at level 7.5. Its horizon
is level 7's: a horizon's depth can't be a fraction, and the chance jumps where the depth
changes.

```rust,ignore
{{#include ../../crates/engine/src/levels.rs:level_for_elo}}
```

The range runs from level 0's rating to level 19's: **600 to 2500**. CNP's `engine/levels`
names the scale "Lichess rapid, estimated from how often people blunder", so a client can say
what the numbers mean.

It is an estimate from blunders alone. A level's other moves are an engine's, its inaccuracies
and mistakes fewer than people's at the low end, and the counts come from Lichess's own
analysis for people but from a search of 100,000 nodes for the levels. Only games against
people would rate the levels for sure: as a Lichess bot, say (`docs/lichess.md`).

## Calibration against engines (until cjus/chessgui#113)

Before the human scale, the levels were **calibrated** against engines, as the PRD then
required: each played games against reference engines with known ratings on the CCRL list,
and `UCI_Elo` was fitted to the results. Those ratings were for the levels without a person's
horizon; they no longer hold, and are kept here as the record.

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
gains from more time much as its opponents do. Level 19 played about 300 below it. The 40+0.4
column was the code's table, and `UCI_Elo` ran from level 0's rating to level 19's, rounded
inward to 50: 650 to 2450 (1000 to 2400 before any calibration, 800 to 2600 after the first).

Before any calibration, the levels followed a guessed straight line from 1000 at level 0 to
2400 at level 19. That was about 370 too high at the bottom and close at the top. A level gained
55 to 75 points up to level 6, where the randomness dominates, then more with each level: about
120 from level 12 up, and 146 from 18 to 19.

With a person's horizon the levels lose far more often to engines, so their ratings against
engines would now be much lower, the lower levels below every reference engine (the weakest is
rated 1058). Rating them on CCRL's scale again would need weaker engines.

## Pitfalls

- **A rating against engines isn't a rating against people.** Engines punish every blunder;
  people miss many. So a level's rating against engines and the rating of the people it plays
  like can be hundreds of points apart, and a level can't be fitted to both (the human scale,
  above).
- **A horizon search must not teach the table.** Its scores are unresolved, evaluated in the
  middle of exchanges; written to the transposition table they would mislead the next full
  search. `Techniques::horizon` leaves the table alone, and runs on one thread.
- **Blunders come in bunches.** One bad game can hold several, so 40 games of a level carry
  about ± 0.5 blunders in 40 moves of noise. The chances are smoothed across levels rather
  than set level by level from one run.

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

At full strength nothing changes (the bench is unchanged). The levels blunder as people of
Lichess rapid ratings 600 to 2500 do, within the noise of the measurement, where before even
level 0 blundered less than people rated 600 and level 18 not at all. `UCI_Elo` means a rating
on that human scale, estimated from blunders; against engines the levels' old CCRL ratings no
longer hold.

## Further reading

- Chess Programming Wiki: [Strength](https://www.chessprogramming.org/Strength),
  [Multi-PV](https://www.chessprogramming.org/Principal_Variation#Multi-PV).
- Lichess, [accuracy and win percentage](https://lichess.org/page/accuracy), and its
  [open database](https://database.lichess.org/) of games.
