# Status and next steps

The single place that says where cb2026 stands: what is merged, what is built and waiting, what
is blocked on whom, and how to carry on. **Claude: read this first in a new session.** Update
it in every PR that changes the picture (a stage merged, a result in, a new blocker).

*Last updated 2026-10-19, 10:56 MDT (M5: the check extension, IIR and three versions of
improving merged, with chapter 14.1; improving is off by default and set aside; time losses not
reproduced, #66; continuation history merged, +10.7 ± 7.3, #69; history in LMR merged off by
default after no verdict, #70; singular extensions and multi-cut merged, +29.5 ± 13.3, #71;
adaptive time management merged, +33.5 ± 14.3, #72; history off loses −17.7 ± 9.9, so it stays
on, #73; history in LMR's second version merged, +22.2 ± 11.4, #74; adaptive time off at 40+0.4
loses −13.8 ± 8.6, so the gain holds at the long time control, #76; negative extensions merged,
+4.9 ± 4.0, #77; SPRT progress comments, #79; singular extensions from depth 6 merged, +5.0 ±
4.1, #78; singular extensions from depth 4 merged, +6.1 ± 4.8, #80; `singular_margin` 3
merged, +5.2 ± 4.2, #82; `singular_margin` 4 failed, −1.8 ± 5.3, so it stays at 3, #84;
transposition table buckets failed, −8.8 ± 8.6, so they are off by default, #86; engine
failures in matches can no longer count as draws, #91). 2026-10-17: one `Game` type in
`cb2026-chess` decides when every game is over, for the shell, the match tool, self-play and
chessgui, #94, merged; buckets at 40+0.4 about even, +1.6 ± 6.3 with no verdict, so they stay
off, #92; the gauntlet reference engines chosen, built by `scripts/fetch-gauntlet-engines.sh`,
#36; double extensions merged off by default after no verdict, +3.6 ± 4.9, #96; the M2/M3
gauntlets running on the M3, #36; **M2 passed**, stage 14 rated 2442 ± 11 by gauntlet, #36.
2026-10-18: **M3 passed**, stage 16 rated 2669 ± 10, #36; `main` rated 2714 ± 10, #36; five
engines from 1058 to 1794 chosen for the level calibration, #101, and `scripts/mac-levels.sh`
written, #104; the levels calibrated, 752–2599, and `UCI_Elo` fitted to them, #103;
`UCI_Elo` widened to 800–2600, #106; at 40+0.4 levels 4, 10 and 16 rated 111–272 lower, so
the levels were recalibrated at 40+0.4, #106. 2026-10-19: the levels at 40+0.4 rated 629–2483,
`levels::RATINGS` set to them and `UCI_Elo` narrowed to 650–2450, #106; `Position::parse_move`
reads a move in SAN or coordinates and says why text names none, including a promotion
without its piece, for the shell and Solrac's chat, #113; correction history merged, +28.0 ±
13.0, #112.*

## Stages

`docs/PRD.md`, "Stages", has the plan. Merged means merged into `main` through its PR.

| Stage | State | PR / branch | Bench | Strength (SPRT at 8+0.08 unless noted) |
| --- | --- | --- | --- | --- |
| 1–10 | Merged | #16–#32 | | `docs/elo-ledger.md` |
| 11 PVS, aspiration | Merged | #33 | 248143 | +28.5 ± 13.1 (Mac, 2001 games) |
| 12 Null move | Merged | #41 | 209085 | +170 ± 40 |
| 13 LMR, LMP | Merged | #42 | 117856 | +242 ± 59 |
| 14 SEE and pruning | Merged | #43 | 1565035 (bench depth 10 from here) | +132 ± 33 |
| 15 Full HCE | Merged | #44 | 1200673 | +192 ± 45 |
| 16 Texel tuning | Merged | #45 | 1200673 | Tuned values −1 ± 21 (no verdict): **not adopted**, hand-set values stay the default |
| 17 Lazy SMP | Merged | #46 | 1207153 | One thread vs stage 16, [−5, 0]: +0.1 ± 4.1, 15,188 games, passed. 4 threads vs 1, [0, 25]: +191 ± 99, 38 games, passed (both on the Mac) |
| 18 Syzygy, Chess960 starts | Merged | #49 | 1207153 | No functional change |
| 18 Polyglot books | Merged | #50 | 1207153 | No functional change (book off by default) |
| 19 Levels, MultiPV | Merged | #51 | 1207153 | No functional change at full strength; levels not yet calibrated (#36) |
| 20 XBoard, pondering, conformance tests, Lichess docs | Merged | #52 | 1207153 | No functional change |
| 21 NNUE | Not started | | | M6 stretch goal |

### Built on top of stage 20 (not stages)

| Work | PR / branch | Bench | State |
| --- | --- | --- | --- |
| Shell: `stats`, `tree`, `explain`, `eval diff`, `see`, `techniques`, `ab` | #53 | 1207153 | Merged, no functional change |
| Shell: `redo`, clocks, `hint`, `resign`, `draw`, PGN save/load | #54 | 1207153 | Merged, no functional change |
| Shell: `match`, `selfplay`, `uci`, `xboard` | #55 | 1207153 | Merged, no functional change |
| M5: check extensions | #57 | 1207153 | Merged, off by default: −8.3 ± 17.7 after 840 games (no gain) |
| M5: internal iterative reductions | #58 | 765380 | Merged: +19.2 ± 10.4, 2,337 games, [0, 5] passed (Mac) |
| M5: improving heuristic | #60 | 765380 | Merged, off by default: −32.0 ± 15.1, 1,142 games, [0, 5] failed (Mac) |
| M5: improving, half a ply off the margin; root evaluation fix | #63 | 765380 | Merged, off by default: −17.7 ± 11.5, 1,691 games, [0, 5] failed (M3 Max) |
| M5: improving, the reverse futility margin only | #65 | 765380 | Merged, off by default: −33.0 ± 15.3, 1,036 games, [0, 5] failed (M3 Max) |
| M5: continuation history (chapter 10.1) | #69 | 754226 | Merged: +10.7 ± 7.3, 4,532 games, [0, 5] passed (M3 Max) |
| M5: history in late move reductions (chapter 14.1) | #70 | 754226 | Merged, off by default: +2.1 ± 5.0, 8,910 games, [0, 5] no verdict (LLR −0.30), stopped (M3 Max) |
| M5: singular extensions and multi-cut (chapter 14.1) | #71 | 1115153 | Merged: +29.5 ± 13.3, 1,357 games, [0, 5] passed (M3 Max) |
| M5: adaptive time management (chapter 6.1) | #72 | 1115153 (bench is depth-limited; timed games change) | Merged: +33.5 ± 14.3, 1,062 games, [0, 5] passed (M3 Max) |
| M5 experiment: history heuristic off (chapter 10.1) | #73 | 1115153 | Merged as a record only: history off −17.7 ± 9.9, 2,124 games, [−5, 0] failed (M3 Max); history stays on |
| M5: history in LMR, second version (chapter 14.1) | #74 | 1126902 | Merged: +22.2 ± 11.4, 1,661 games, [0, 5] passed (M3 Max) |
| M5 experiment: adaptive time off at 40+0.4 (chapter 6.1) | #76 | 1126902 | Merged as a record only: adaptive time off −13.8 ± 8.6, 2,599 games, [−5, 0] at 40+0.4 failed (M3 Max); adaptive time stays on |
| M5: negative extensions (chapter 14.1) | #77 | 1100720 | Merged: +4.9 ± 4.0, 14,211 games, [0, 5] passed (M3 Max) |
| M5: singular extensions from depth 6 (chapter 14.1) | #78 | 1131028 | Merged: +5.0 ± 4.1, 12,282 games, [0, 5] passed (M3 Max) |
| M5: singular extensions from depth 4 (chapter 14.1) | #80 | 1343312 | Merged: +6.1 ± 4.8, 9,057 games, [0, 5] passed (M3 Max) |
| M5: `singular_margin` 3 (chapter 14.1) | #82 | 1098926 | Merged: +5.2 ± 4.2, 11,400 games, [0, 5] passed (M3 Max) |
| M5: `singular_margin` 4 (chapter 14.1) | #84 | 1098926 | Merged as a record, the margin back to 3: −1.8 ± 5.3, 7,476 games, [0, 5] failed (M3 Max) |
| M5: transposition table buckets (chapter 8.1) | #86 | 1098926 (1114877 with buckets on) | Merged, off by default: −8.8 ± 8.6, 2,685 games, [0, 5] failed (M3 Max) |
| M5: transposition table buckets at 40+0.4 (chapter 8.1) | #92 | 1098926 (1114877 with buckets on) | Merged as a record, off by default: +1.6 ± 6.3, 4,630 games, [0, 5] at 40+0.4 no verdict (LLR −0.45), stopped (M3 Max) |
| `cb2026_chess::game::Game`: one record of a game and one set of end-of-game rules, moved from chessgui and used by the shell, the match tool and self-play | #94 | 1098926 | Merged, no functional change; chessgui uses it from cjus/chessgui#31 |
| M5: double extensions (chapter 14.1) | #96 | 1098926 (1248666 with double extensions on) | Merged, off by default: +3.6 ± 4.9, 8,670 games, [0, 5] no verdict (LLR 0.92), stopped (M3 Max) |
| `Position::parse_move`: a move typed in SAN or coordinates, and `SanError::MissingPromotion` when a pawn promotes without its piece; the shell uses it, and Solrac's chat will (cjus/chessgui#38) | #113 | 1098926 | Merged, no functional change |
| M5: correction history (chapter 14.2) | #112 | 1065754 | Merged: +28.0 ± 13.0, 1,308 games, [0, 5] passed (M3 Max) |

**Nothing is running on the M3.** Correction history's SPRT (#112) finished on 2026-10-19 at
10:53 MDT. It started at about 09:45 and passed in 1,308 games: **+28.0 ± 13.0**, about as much
as singular extensions and multi-cut (#71). It is on by default, and the M3's next job is
another M5 SPRT (item 3 below).

The recalibration of the levels at 40+0.4 (#106) finished on
2026-10-19 at 08:28 MDT. It started at about 21:45 MDT the evening before, and rated levels 0,
2, 6, 8, 12, 14, 18 and 19, 6,400 games in under 11 hours. With levels 4, 10 and 16 from the
check before it (2,400 games), Skill Levels 0–19 rate **629 to 2483** at 40+0.4 (± 23 to 55),
74 to 272 below the 8+0.08 calibration. The odd levels are halfway between their neighbours.
`levels::RATINGS` holds the new table, and `UCI_Elo` runs from level 0's rating to level 19's,
rounded inward to 50: **650–2450** (levels 0.3 to 18.8). The levels search a fixed number of
nodes, so they play the same at any time control, while the reference engines play better with
more time. CCRL rated those engines at 2'+1", slower than both, so 40+0.4 is the better
estimate. Walleye 1.3.0 lost 8 games by an illegal move (its en passant bug), and Arche 0.3.8 2
by answering `bestmove 0000` (no move), a bug of its own.

The level calibration (#103) finished on 2026-10-18 at about
16:10 MDT, 16,000 games in six and a half hours: Skill Levels 0–19 rated **752 to 2599**, and
`UCI_Elo` now maps onto those ratings, interpolating between levels (`levels::RATINGS`,
`level_for_elo`, `settings_at`; chapter 19). Walleye 1.3.0 lost about 1% of its games by an
illegal move: it ignores en passant captures in the moves it is sent, a bug of its own.
`UCI_Elo` became 800–2600 (levels 0.7 to 19.0), widened from 1000–2400 (Carlos, #106). The
recalibration at 40+0.4 replaced both.

The rating gauntlets (#36) finished on 2026-10-18 at 06:50 MDT, 800 games against each of the
seven reference engines at 8+0.08, about 4 hours a version: stage 14 **2442 ± 11** (M2 passed),
stage 16 **2669 ± 10** (M3 passed), and `main` **2714 ± 10**, all recorded in the PRD's
"Milestones" and the ledger. `main` is only 45 ± 14 above stage 16, though the M5 SPRTs that
passed add up to about +136 in self-play: self-play overstates gains against other engines about
threefold here.

When several PRs wait for the M3, they run one at a time, and **before each SPRT starts, rebase
that PR onto the current `main`** and re-measure its `Bench:` line: `scripts/mac-sprt.sh` plays
the PR's head against `main`, so an unrebased PR would be measured against changes it doesn't
have. For each: if it passes, record it in its chapter and the ledger, mark it ready and merge;
if it fails, switch it off by default (as with #57, #60, #63 and #65), record it and merge.

**The SPRT loop with Carlos.** Claude opens the PR as a draft with the M3 command in its
description, ending with `tail -f ~/sprt-<N>.log` so Carlos can watch it (Carlos asked for
that on every job, 2026-10-15). Carlos starts it on the M3 and says so ("SPRT started on the
M3 for #N"). Claude then runs `scripts/watch-sprt.sh <N>` in the background: it checks the PR every 10 minutes for
the comment `mac-sprt.sh` posts and prints it (Carlos approved this polling on 2026-10-12).
`mac-sprt.sh` also keeps a progress comment on the PR, updated every 2,000 games (Carlos,
2026-10-14). If it shows 8,000 games or more with no result, the watcher stops early (exit
3), and Claude proposes stopping the run to Carlos; to keep watching after that, run it again
with a higher threshold (`scripts/watch-sprt.sh <N> 24 12000`) or none (`… 24 0`).
On the result, Claude records it and merges, then rebases the next PR in the queue and tells
Carlos it's ready. If the watcher gives up, ask Carlos for `tail -5 ~/sprt-<N>.log` and
`pgrep -fl "match|mac-sprt"` on the M3.

**En Croissant on the M1 Max** uses `~/engines/cb2026`, a copy of a release build of `main`
made on 2026-10-12 at 33d37e8, before the M5 gains of 2026-10-13. To refresh it when Carlos
asks: build `main` (`cargo build --release`), check `bench` matches `main`'s, then
`cp target/release/cb2026 ~/engines/cb2026`.

## Milestones

`docs/PRD.md`, "Milestones", defines the gates and records the results. M0, M2 and M3 are
passed; the rest wait on GUI checks, level calibration or the v1.0 work.

| Milestone | Scope | State |
| --- | --- | --- |
| M0 Foundations | stages 1–3 | **Passed** 2026-10-11: perft suites exact, 278.8 Mnps on the M1 Max (#20); 369.6 on the M3 Max |
| M1 Playable | stages 4–7 | Merged; GUI check still open (#24) |
| M2 Classical search | stages 8–14 | **Passed** 2026-10-18: stage 14 rated 2442 ± 11 by gauntlet at 8+0.08 (gate ~2200 CCRL), #36 |
| M3 Evaluation | stages 15–16 | **Passed** 2026-10-18: stage 16 rated 2669 ± 10 by gauntlet at 8+0.08 (gate ≥2400 CCRL), #36 |
| M4 Complete engine | stages 17–20 | Merged (#46–#55); levels calibrated at 40+0.4 on 2026-10-19, 629–2483 (#106); GUI matrix (#37) open |
| M5 v1.0 | strength work, release | Started (the M5 branches above); release workflow merged (#40); `main` rated 2714 ± 10 by gauntlet on 2026-10-18, above the gate (≥2400, goal 2600+), #36 |
| M6 NNUE | stage 21 | Not started |

## Waiting on Carlos

The open [`needs-carlos` issues](https://github.com/cjus/chessengine/issues?q=is%3Aopen+label%3Aneeds-carlos):

- #37: GUI compatibility matrix; Lichess bot account.
- #24 (M1 GUI games).

On hold: #61, running long tests on AWS Batch (closed 2026-10-12; reopen it if the M3 can't keep
up with the test queue).

Done on 2026-10-18: #36 (the M2/M3 gauntlets and one on `main`, run overnight on the M3:
2442, 2669 and 2714), #101 (five engines from 1058 to 1794 for the levels) and #103 (the
level calibration, 752–2599).

Done on 2026-10-19: #106 (the levels recalibrated at 40+0.4, 629–2483, and `UCI_Elo` 650–2450).

Done on 2026-10-12: #62 (the M3 Max is set up; its check: `bench` 765380, the perft suites
exact at 369.6 Mnps) and #66 (the time-loss checks on the M3: none).

Done on 2026-10-11: #17 (stage tags 1–16 pushed from the Mac; Claude now pushes each new stage
tag itself when it runs there, and has tagged stages 17–20), #6 (the Cross-platform workflow
passed on macOS and Windows), #47 (both stage 17 tests passed) and #20 (the M0 perft gate passed;
Claude ran it on the idle Mac).

## How to carry on

### Merging the stack

For each branch of a stack (none is open now), once it has its SPRT result and its book
section:

1. Rebase the branch onto the latest `origin/main`. The commits below it are already in `main`,
   so only its own commits replay.
2. Open its PR. Then commit the result to the chapter's "What it's worth"
   table and a `docs/elo-ledger.md` row with the PR number, as a separate commit.
3. Wait for the three CI checks, then merge (CLAUDE.md: Claude may merge its own green PRs).
   Push the stage's tag (`git tag -a stage-NN-<name> <merge commit> -m "..."`); from a cloud
   session, which can't push tags, add the command to a `needs-carlos` issue instead.
4. Rebase the rest of the stack onto the new branch head, and point each branch at its rebased
   commit (match by commit subject). Push with `--force-with-lease`.

When a change below a commit alters the search, every later commit's `Bench:` line must be
re-measured:

```sh
git rebase --onto <new base> <old base> <branch> \
  --exec 'cargo build --release -q -p cb2026 && b=$(./target/release/cb2026 bench | awk "/Nodes searched/{print \$4}") && git commit -q --amend -m "$(git log -1 --format=%B | sed "s/^Bench: [0-9]*\$/Bench: $b/")" && cargo test -q --workspace'
```

Run the full test suite on every commit you replay, not only at the tip: each becomes a PR head.

### Testing strength

- **The cloud machine** (4 cores) runs short SPRTs, 3 games at a time, but a background job
  stops after 2 hours. That's about 850 games, enough for gains of 30+ Elo. Smaller changes need
  the test Mac.
- **The test Mac** is, from 2026-10-12, an M3 Max on Carlos's network: 10 performance and 4
  efficiency cores, 36 GB (#62; the PRD's "Test machine" section has its speed). Carlos starts
  long tests there (`docs/testing.md`, "Long tests on the test Mac"), and the scripts post the
  results to the PR. `scripts/mac-sprt.sh <PR> [elo0 elo1]` runs one game per performance core
  minus one: 9 on the M3 Max, about 1,250 games an hour at 8+0.08 (the M1 Max, 7 at a time,
  played about 970; a game takes about 26 seconds whatever the CPU, because the clock sets its
  length). CLAUDE.md says long SPRTs and gauntlets are Carlos's to run.
- **The M1 Max is no longer a test machine.** Claude Code runs on it, Carlos needs it free and
  disconnects it, so don't start SPRTs, gauntlets or other long runs there.
- **Neutral changes take long.** When the true difference sits at one of the two hypotheses, the
  SPRT needs many games: stage 17's one-thread test (about 0 Elo, bounds [−5, 0]) took 15,188
  games, 15 hours on the M1 Max. Clear gains of +100 Elo or more end in a few hundred. A change
  worth about half the upper bound is the slowest of all: #70 (history in LMR) sat at +2 ± 5
  after 8,910 games and was stopped with no verdict (Carlos, 2026-10-13) and merged off. If an
  SPRT is still undecided after about 8,000 games with its estimate between the bounds,
  propose the same. It is a proposal, not a rule: #77 (negative extensions) was still going at
  8,000 games and passed at 14,211 with +4.9 ± 4.0, #80 (singular extensions from depth 4),
  which Carlos let run on, passed at 9,057 with +6.1 ± 4.8, and #82 (`singular_margin` 3),
  at +4.1 ± 5.0 after 8,000, passed at 11,400 with +5.2 ± 4.2.
- **The M3's pace varies.** #69 played about 1,390 games an hour; #70 averaged about 760 an
  hour overnight. Something slowed the M3 that night. Since it has a lid, closing it would make
  the Mac sleep even with `caffeinate -i`. Expect fewer games an hour than the estimate above
  when the M3 is in use or asleep for part of the run.
- **Thread scaling** (`scripts/mac-smp.sh`) plays one game at a time, about 140 an hour, so its
  bounds default to [0, 25]: it asks whether more threads clearly gain. Stage 17's 4-thread test
  passed in 38 games, 17 minutes.
- **Engine failures in a match** (#89, sections A and B, fixed 2026-10-16): a crash or hang
  loses the game and that worker starts both engines again; a game that can't start is
  aborted, listed (result `*`) and not scored; and a run that breaks posts **SPRT run failed**
  with the end of its log, never a quiet result. Before, a dead engine's later games counted as
  draws. Every SPRT comment from #69 on lists 0 failures for both engines, so those results
  weren't affected; the earlier comments (#46–#65) didn't list failures. The rest of #89
  (search and protocol findings) is still to review.
- **Bounds:** gainers use [0, 5]; refactors and anything meant to be neutral use [−5, 0]. A
  result without a verdict is recorded honestly. Stage 16, the check extension and improving
  went in switched off rather than claimed as gains.

### Next work, in PRD order

1. Time losses: **not reproduced** (2026-10-12, #66). IIR's SPRT had two in 2,337 games at 8+0.08
   on the M1 Max, one for each engine. Since then there have been none:

   | Run | Machine | Games | Time losses |
   | --- | --- | ---: | --- |
   | #60's SPRT, 8+0.08 | M1 Max, 7 at a time | 1,142 | 0 |
   | #63's SPRT, 8+0.08 | M3 Max, 9 at a time | 1,691 | 0 |
   | #65's SPRT, 8+0.08 | M3 Max, 9 at a time | 1,036 | 0 |
   | Stress test, `main` vs itself, 1+0.01 | M3 Max, 9 at a time | 2,000 | 0 (and no illegal moves or failures) |

   cb2026 never plans more than half its remaining clock and checks the clock every 1,024 nodes,
   so those two losses needed a stall of 200 ms or more: most likely the M1 Max, which was also
   Carlos's working machine. Nothing to fix now. If a loss happens again, #67's details (move,
   time taken, clock) say which kind it is, and every SPRT comment lists them. The M5 robustness
   run (10,000+ games at 1+0.01, 10+0.1 and 40/60) is the final check.
2. Milestone gates: the reference engines are chosen (#36, 2026-10-17; `docs/PRD.md`,
   "Testing and strength measurement"), and `scripts/fetch-gauntlet-engines.sh` builds them. Carlos
   started the M2 and M3 gauntlets on the M3 on 2026-10-17 (the stage 14 and 16 tags), then one
   on `main`. Stage 14 rated 2442 ± 11, so M2 passed; stage 16 2669 ± 10, so M3 passed; and
   `main` 2714 ± 10. The levels are calibrated too (#103): five weaker engines (1058–1794,
   #101) joined the seven in `levels.txt`, `scripts/mac-levels.sh` rated each Skill Level
   against the four nearest it, from 752 (level 0) to 2599 (level 19), and `UCI_Elo` maps onto
   those ratings. The straight line it replaced was too flat: about 250 too high at the bottom,
   200 too low at the top. At 40+0.4 the levels rated 74–272 lower, so they were recalibrated
   there (#106): 629 to 2483, and `UCI_Elo` 650–2450.
3. More M5 strength work, each change with an SPRT. Passed on 2026-10-13: continuation history
   (#69, +10.7), singular extensions and multi-cut (#71, +29.5), adaptive time management (#72,
   +33.5), history in LMR's second version (#74, +22.2); on 2026-10-14, negative extensions
   (#77, +4.9 ± 4.0, after 14,211 games and about 13 hours); on 2026-10-15, singular extensions
   from depth 6 (#78, +5.0 ± 4.1, 12,282 games, about 10 hours) and from depth 4 (#80, +6.1 ±
   4.8, 9,057 games, about 7 hours); on 2026-10-16, `singular_margin` 3 (#82, +5.2 ± 4.2,
   11,400 games, about 9 hours); on 2026-10-19, correction history (#112, +28.0 ± 13.0, 1,308
   games, about 70 minutes). Failed, so not adopted: `singular_margin` 4 (#84, −1.8 ± 5.3
   against 3, 7,476 games, about 6 hours) and transposition table buckets (#86, −8.8 ± 8.6,
   2,685 games, about 2 hours; off by default). No verdict, off: history in
   LMR's first version (#70) and, on 2026-10-17 at 40+0.4, transposition table buckets (#92,
   +1.6 ± 6.3 over 4,630 games, about 18 hours, stopped) and, at 8+0.08, double extensions
   (#96, +3.6 ± 4.9 over 8,670 games, about 7 hours, stopped). Failed as a simplification, so kept:
   history off (#73, −17.7) and, on
   2026-10-14 at the long time control 40+0.4, adaptive time off (#76, −13.8 ± 8.6 over 2,599
   games; about 255 games an hour on the M3, 10 hours).

   Next, in a rough order of value:
   - **Capture history and ProbCut**, both new techniques and usually worth more than the
     parameter changes below. Correction history passed (#112, +28.0); more correction tables,
     keyed by pieces other than the pawns as several strong engines do, could follow it.
   - **Tuning singular extensions, continued**: depth 6 passed (#78) and depth 4 too (#80),
     which is about where Stockfish starts; `singular_margin` 3 passed too (#82), and 4
     failed (#84). That settles the start depth and the margin for now. Double extensions,
     which extend a move two plies when it is singular by a wide margin (as in Stockfish,
     GPL-3.0), had no verdict (#96, +3.6 ± 4.9) and are off; next for them, a margin of 50
     (the depth-10 tree 30% bigger instead of 44%), a limit of 3 (23%), or 40+0.4. Still
     untried: the hash entry's depth margin (`singular_tt_depth_margin`, 3). Gains this size
     take 9,000–14,000 games, 7–13 hours on the M3.
   - **Tuning adaptive time at 40+0.4**: the gain there (about 14 Elo) is smaller than at
     8+0.08 (about 33), and the scale's numbers were set by hand. Each run at 40+0.4 takes
     about 10 hours, so it waits behind cheaper candidates.
   - **Buckets with a smaller `Hash`** (low value): off after #86 lost at 8+0.08 and #92 was
     about even at 40+0.4. A small table at 8+0.08 would test a full table cheaply, but a gain
     there would say little about the default 16 MB.

   Improving is set aside (Carlos, 2026-10-12) after three failed versions (#60, #63, #65). The results point at
   reverse futility pruning, and the late-move-pruning part may have helped, so later
   candidates are improving in late move pruning only (`rfp_improving` = 0, `lmp_improving` =
   2) and a wider `rfp_margin`.
4. The v1.0 release (`docs/releasing.md`), then stage 21 (NNUE).
