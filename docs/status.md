# Status and next steps

The single place that says where cb2026 stands: what is merged, what is built and waiting, what
is blocked on whom, and how to carry on. **Claude: read this first in a new session.** Update
it in every PR that changes the picture (a stage merged, a result in, a new blocker).

*Last updated 2026-10-11 (stages 17–20 and the shell work merged; Claude Code on Carlos's Mac).*

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
| M5: check extensions | `claude/m5-check-extension` | 1207153 | Off by default: −8.3 ± 17.7 after 840 games (no gain). Waits for its book chapter (CLAUDE.md, definition of done, item 5) |
| M5: internal iterative reductions | `claude/m5-iir` | 765380 | SPRT vs stage 20: about +20 to +28 (± 29) after about 300 games when the cloud session ended, no verdict; **rerun it** |
| M5: improving heuristic | `claude/m5-improving` | 648634 | Not tested yet (test it against IIR) |

The three M5 branches form one linear stack on `main`, in the order of the table: each is the one
above it plus one commit. `claude/m5-improving` is the tip.

## Milestones

`docs/PRD.md`, "Milestones", defines the gates and records the results. M0 is passed; the rest
wait on rating gauntlets or GUI checks.

| Milestone | Scope | State |
| --- | --- | --- |
| M0 Foundations | stages 1–3 | **Passed** 2026-10-11: perft suites exact, 278.8 Mnps on the M1 Max (#20) |
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

Done on 2026-10-11: #17 (stage tags 1–16 pushed from the Mac; Claude now pushes each new stage
tag itself when it runs there, and has tagged stages 17–20), #6 (the Cross-platform workflow
passed on macOS and Windows), #47 (both stage 17 tests passed) and #20 (the M0 perft gate passed;
Claude ran it on the idle Mac).

## How to carry on

### Merging the stack

For each branch in order (the check extension, IIR, improving), once it has its SPRT result and
its book chapter:

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
git rebase --onto <new base> <old base> claude/m5-improving \
  --exec 'cargo build --release -q -p cb2026 && b=$(./target/release/cb2026 bench | awk "/Nodes searched/{print \$4}") && git commit -q --amend -m "$(git log -1 --format=%B | sed "s/^Bench: [0-9]*\$/Bench: $b/")" && cargo test -q --workspace'
```

Run the full test suite on every commit you replay, not only at the tip: each becomes a PR head.

### Results still to record

- **M5 techniques:** IIR and improving need SPRTs. The check extension's result is in its commit
  and in `crates/engine/src/search.rs`'s module docs. None of the three has a book chapter yet:
  the plan is one chapter on extensions and reductions after stage 14, and the three branches
  merge with it.

### Testing strength

- **The cloud machine** (4 cores) runs short SPRTs, 3 games at a time, but a background job
  stops after 2 hours. That's about 850 games, enough for gains of 30+ Elo. Smaller changes need
  the Mac.
- **On the Mac** that limit doesn't apply. `scripts/mac-sprt.sh <PR> [elo0 elo1]` runs 7 games at
  a time, about 980 games an hour at 8+0.08 (a game takes about 26 seconds whatever the CPU,
  because the clock sets its length), and posts to the PR. CLAUDE.md says long SPRTs and
  gauntlets are Carlos's to run; ask him before starting one, even when Claude is running on
  the Mac.
- **Neutral changes take long.** When the true difference sits at one of the two hypotheses, the
  SPRT needs many games: stage 17's one-thread test (about 0 Elo, bounds [−5, 0]) took 15,188
  games, 15 hours on the Mac. Clear gains of +100 Elo or more end in a few hundred.
- **Thread scaling** (`scripts/mac-smp.sh`) plays one game at a time, about 140 an hour, so its
  bounds default to [0, 25]: it asks whether more threads clearly gain. Stage 17's 4-thread test
  passed in 38 games, 17 minutes.
- **Bounds:** gainers use [0, 5]; refactors and anything meant to be neutral use [−5, 0]. A
  result without a verdict is recorded honestly. Stage 16 and the check extension went in
  switched off rather than claimed as gains.

### Next work, in PRD order

1. M5: rerun IIR's SPRT on the Mac ([0, 5]), then test improving against IIR. Write the chapter
   on extensions and reductions, and merge the check extension, IIR and improving with it.
2. Milestone gates, once #36 is answered: gauntlets for M2–M4, level calibration (the
   calibration script is still to write, using `crates/tools/src/gauntlet.rs`).
3. More M5 strength work, each change with an SPRT. Candidates: continuation history,
   history-based LMR adjustment, singular extensions, time management.
4. The v1.0 release (`docs/releasing.md`), then stage 21 (NNUE).
