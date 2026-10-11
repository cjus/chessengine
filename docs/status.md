# Status and next steps

The single place that says where cb2026 stands: what is merged, what is built and waiting, what
is blocked on whom, and how to carry on. **Claude: read this first in a new session.** Update
it in every PR that changes the picture (a stage merged, a result in, a new blocker).

*Last updated 2026-10-11 (handoff from a Claude cloud session to Claude Code on Carlos's Mac).*

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
| 17 Lazy SMP | **Open, blocked on Carlos** | #46, `claude/stage-17-smp` | 1207153 | Cloud no-regression [−5, 0]: −2.9 ± 17.1, 840 games, no verdict. Mac runs requested in #47 |
| 18 Syzygy, Chess960 starts | Built, behind 17 | `claude/stage-18-syzygy` | 1207153 | No functional change |
| 18 Polyglot books | Built, behind 17 | `claude/stage-18-book` | 1207153 | No functional change (book off by default) |
| 19 Levels, MultiPV | Built, behind 17 | `claude/stage-19-levels` | 1207153 | No functional change at full strength; levels not yet calibrated (#36) |
| 20 XBoard, pondering, conformance tests, Lichess docs | Built, behind 17 | `claude/stage-20-xboard` | 1207153 | No functional change |
| 21 NNUE | Not started | | | M6 stretch goal |

### Built on top of stage 20 (not stages, also waiting)

| Work | Branch | Bench | State |
| --- | --- | --- | --- |
| Shell: `stats`, `tree`, `explain`, `eval diff`, `see`, `techniques`, `ab` | `claude/shell-explain` | 1207153 | Done, no functional change |
| Shell: `redo`, clocks, `hint`, `resign`, `draw`, PGN save/load | `claude/shell-play` | 1207153 | Done, no functional change |
| Shell: `match`, `selfplay`, `uci`, `xboard` | `claude/shell-match` | 1207153 | Done, no functional change |
| M5: check extensions | `claude/m5-check-extension` | 1207153 | Off by default: −8.3 ± 17.7 after 840 games (no gain) |
| M5: internal iterative reductions | `claude/m5-iir` | 765380 | SPRT vs stage 20: about +20 ± 29 after 280 games when the cloud session ended; **rerun it** |
| M5: improving heuristic | `claude/m5-improving` | 648634 | Not tested yet (test it against IIR) |

The branches form one linear stack, in the order of the tables above: each branch is the one
above it plus one or two commits. `claude/m5-improving` is the tip.

## Milestones

`docs/PRD.md`, "Milestones", defines the gates. None is formally passed yet, because the rating
gauntlets haven't run.

| Milestone | Scope | State |
| --- | --- | --- |
| M0 Foundations | stages 1–3 | Merged; perft gate on the M1 Max still open (#20) |
| M1 Playable | stages 4–7 | Merged; GUI check still open (#24) |
| M2 Classical search | stages 8–14 | Merged; gauntlet (~2200 CCRL) waits on #36 |
| M3 Evaluation | stages 15–16 | Merged; gauntlet (≥2400 CCRL) waits on #36 |
| M4 Complete engine | stages 17–20 | Built; GUI matrix (#37) and level calibration (#36) open |
| M5 v1.0 | strength work, release | Started (the M5 branches above); release workflow merged (#40) |
| M6 NNUE | stage 21 | Not started |

## Waiting on Carlos

The open [`needs-carlos` issues](https://github.com/cjus/chessengine/issues?q=is%3Aopen+label%3Aneeds-carlos):

- **#47: blocks everything.** Run `scripts/mac-sprt.sh 46 -5 0` (one-thread no-regression for
  stage 17), then `scripts/mac-smp.sh 46` (4 threads vs 1). #46 merges when the first passes.
- #17: push the stage tags (1–16 so far).
- #36: choose the gauntlet reference engines; run the M2/M3 gauntlets.
- #37: GUI compatibility matrix; Lichess bot account.
- #24 (M1 GUI games), #20 (M0 perft on the Mac), #6 (Cross-platform workflow).

## How to carry on

### Merging the stack, once #46 can merge

For each stage in order (17, 18 part 1, 18 part 2, 19, 20, then the shell branches):

1. Rebase the branch onto the latest `origin/main`. The commits below it are already in `main`,
   so only its own commits replay.
2. Open its PR (or update #46). Then commit the result to the chapter's "What it's worth"
   table and a `docs/elo-ledger.md` row with the PR number, as a separate commit.
3. Wait for the three CI checks, then merge (CLAUDE.md: Claude may merge its own green PRs).
   Add the stage's tag command to #17.
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

- **Chapter 17:** the one-thread result row, and `SMP_RESULT` (the 4-thread result), from #46.
- **Ledger rows** for stages 17–20 when their PRs open. Stages 18–20 make no functional change;
  record them as such.
- **M5 techniques:** IIR and improving need SPRTs. The check extension's result is in its commit
  and in `crates/engine/src/search.rs`'s module docs. None of the three has a book chapter yet:
  the plan is one chapter on extensions and reductions after stage 14.

### Testing strength

- **The cloud machine** (4 cores) runs short SPRTs, 3 games at a time, but a background job
  stops after 2 hours. That's about 850 games, enough for gains of 30+ Elo. Smaller changes need
  the Mac.
- **On the Mac** that limit doesn't apply. `scripts/mac-sprt.sh <PR> [elo0 elo1]` runs 7 games at
  a time and posts to the PR. CLAUDE.md says long SPRTs and gauntlets are Carlos's to run; ask
  him before starting one, even when Claude is running on the Mac.
- **Bounds:** gainers use [0, 5]; refactors and anything meant to be neutral use [−5, 0]. A
  result without a verdict is recorded honestly. Stage 16 and the check extension went in
  switched off rather than claimed as gains.

### Next work, in PRD order

1. Merge stages 17–20 and the shell branches (above).
2. Finish M5's open SPRTs (IIR, then improving), and write their book chapter.
3. Milestone gates, once #36 is answered: gauntlets for M2–M4, level calibration (the
   calibration script is still to write, using `crates/tools/src/gauntlet.rs`).
4. More M5 strength work, each change with an SPRT. Candidates: continuation history,
   history-based LMR adjustment, singular extensions, time management.
5. The v1.0 release (`docs/releasing.md`), then stage 21 (NNUE).
