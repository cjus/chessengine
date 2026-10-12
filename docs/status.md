# Status and next steps

The single place that says where cb2026 stands: what is merged, what is built and waiting, what
is blocked on whom, and how to carry on. **Claude: read this first in a new session.** Update
it in every PR that changes the picture (a stage merged, a result in, a new blocker).

*Last updated 2026-10-12 (M5: the check extension, IIR and two versions of improving merged,
with chapter 14.1; a third version of improving, #65, waits for its SPRT on the M3 Max).*

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
| M5: improving, the reverse futility margin only (on by default) | #65 | 643051 | Built; SPRT on the M3 Max to run |

One M5 branch is open: `claude/m5-improving-rfp` (#65), one commit on top of `main`, waiting
for its SPRT on the M3 Max.

## Milestones

`docs/PRD.md`, "Milestones", defines the gates and records the results. M0 is passed; the rest
wait on rating gauntlets or GUI checks.

| Milestone | Scope | State |
| --- | --- | --- |
| M0 Foundations | stages 1–3 | **Passed** 2026-10-11: perft suites exact, 278.8 Mnps on the M1 Max (#20); 369.6 on the M3 Max |
| M1 Playable | stages 4–7 | Merged; GUI check still open (#24) |
| M2 Classical search | stages 8–14 | Merged; gauntlet (~2200 CCRL) waits on #36 |
| M3 Evaluation | stages 15–16 | Merged; gauntlet (≥2400 CCRL) waits on #36 |
| M4 Complete engine | stages 17–20 | Merged (#46–#55); GUI matrix (#37) and level calibration (#36) open |
| M5 v1.0 | strength work, release | Started (the M5 branches above); release workflow merged (#40) |
| M6 NNUE | stage 21 | Not started |

## Waiting on Carlos

The open [`needs-carlos` issues](https://github.com/cjus/chessengine/issues?q=is%3Aopen+label%3Aneeds-carlos):

- #36: choose the gauntlet reference engines; run the M2/M3 gauntlets.
- #37: GUI compatibility matrix; Lichess bot account.
- #24 (M1 GUI games).

On hold: #61, running long tests on AWS Batch (closed 2026-10-12; reopen it if the M3 can't keep
up with the test queue).

Done on 2026-10-12: #62 (the M3 Max is set up; its check: `bench` 765380, the perft suites
exact at 369.6 Mnps).

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
  games, 15 hours on the M1 Max. Clear gains of +100 Elo or more end in a few hundred.
- **Thread scaling** (`scripts/mac-smp.sh`) plays one game at a time, about 140 an hour, so its
  bounds default to [0, 25]: it asks whether more threads clearly gain. Stage 17's 4-thread test
  passed in 38 games, 17 minutes.
- **Bounds:** gainers use [0, 5]; refactors and anything meant to be neutral use [−5, 0]. A
  result without a verdict is recorded honestly. Stage 16, the check extension and improving
  went in switched off rather than claimed as gains.

### Next work, in PRD order

1. Improving failed twice (#60: −32 Elo; #63, half a ply off the margin: −18). Both versions
   also doubled late move pruning's threshold when improving. #65 tests the margin on its own
   (late move pruning unchanged), on the M3 Max: `scripts/mac-sprt.sh 65`. If it passes, record
   the result in chapter 14.1 and the ledger and merge it. If it fails, switch improving off
   again and set it aside (Carlos, 2026-10-12).
2. Time losses: IIR's SPRT had two in 2,337 games at 8+0.08, one for each engine, so not caused by
   IIR (the referee allows 50 ms over the clock); improving's 1,142 games had none. Find the
   cause before the M5 robustness run, which must be clean over 10,000 games.
3. Milestone gates, once #36 is answered: gauntlets for M2–M4, level calibration (the
   calibration script is still to write, using `crates/tools/src/gauntlet.rs`).
4. More M5 strength work, each change with an SPRT. Candidates: continuation history,
   history-based LMR adjustment, singular extensions, time management.
5. The v1.0 release (`docs/releasing.md`), then stage 21 (NNUE).
