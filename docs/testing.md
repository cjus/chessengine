# Testing cb2026

How to check that a change is correct, and how to measure whether it makes the engine
stronger. See `docs/PRD.md`, "Testing and strength measurement", for the rules.

## Correctness

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                     # unit tests, perft up to 1M nodes per depth
cargo run --release -p cb2026-tools --bin perft-suite   # every perft depth, with speed
```

CI runs all of these on every pull request.

## Fuzzing

The parsers that read outside input (FEN, SAN and UCI moves, whole UCI sessions) are fuzzed
with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz): random inputs, guided by coverage,
looking for panics, hangs and broken round trips. It needs nightly Rust:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run fen -- -max_total_time=60      # also: moves, uci
```

The `Fuzz` workflow runs each target for five minutes every night. A crash leaves the input in
`fuzz/artifacts/`; turn it into a regular test when fixing it.

## Strength: the `match` tool

`match` plays two UCI engines against each other and reports the score, an Elo estimate with
its 95% error margin, and (with `--sprt`) a sequential probability ratio test.

```sh
cargo build --release
cp target/release/cb2026 /tmp/cb2026-old    # build and save the version to compare against
# ...make the change...
cargo build --release
cargo run --release -p cb2026-tools --bin match -- \
    --engine target/release/cb2026 --engine /tmp/cb2026-old \
    --tc 8+0.08 --concurrency 3 --sprt 0 5 --pgn /tmp/games.pgn
```

- **`--tc`**: `8+0.08` (seconds + increment, the short time control), `40+0.4` (long),
  `40/60` (40 moves in 60 seconds, repeating: tests `movestogo`), `nodes=20000` or `depth=4`
  (machine-independent, good for quick checks).
- **`--sprt elo0 elo1`**: `0 5` for a change meant to gain strength, `-5 0` for a
  simplification that must not lose any. The match stops when the test passes or fails.
- **`--concurrency`**: games at a time. Use the number of cores minus one; in a 4-core cloud
  session, 3.
- **`--openings`**: defaults to `tests/data/openings.epd` (50 positions, each played with both
  colours).
- **`--option Name=Value`** sets a UCI option on both engines; **`--first-option`** only on
  the first (used for thread-scaling tests).
- **Exit status**: 0 if the SPRT passed, 1 if it failed or was inconclusive, or if no game
  was scored; 2 if the match couldn't be played in full (an engine that won't start, a bad
  or already-drawn opening, a worker that failed). A panic aborts a release build, with
  status 134.

**When an engine fails** (cjus/chessengine#89): a crash or hang during a game loses it, and that
worker starts both engines again before its next game, so a dead or hung process never plays
on. A game that can't start (no answer to `ucinewgame`) is **aborted**: it is listed and saved
with the result `*`, but not scored, since a draw would add false evidence to the test. Three
aborted games in a row on one worker stop the match with an error, as does an engine that can't
be started again.

## Robustness runs

The PRD asks for zero illegal moves and crashes over 10,000+ games per milestone, and zero time
losses at 1+0.01, 10+0.1 and 40/60 repeating, with the default `Move Overhead`. The `match`
tool's last lines count the games that ended badly, by the engine at fault:

```text
Problems for target/release/cb2026: 0 time losses, 0 illegal moves, 0 failures, 0 aborted games
```

followed by the reasons for the first few. A time loss names the move, how long the engine
took and what was on its clock:

```text
  White (cb2026 0.1.0) lost on time at move 87: took 412 ms with 260 ms on its clock
```

A little over the clock points at the engine's time management or its `Move Overhead`; hundreds
of milliseconds over points at the machine stalling the process. (cb2026 never plans more than
half its remaining clock, so its own losses should be the second kind.) Save the games with
`--pgn <file>` to look at the ones that went wrong.

A robustness run is a long match at each of those time controls (`--tc 1+0.01`, `--tc 10+0.1`,
`--tc 40/60`), for example against the previous release.

```sh
## Quick matches from the shell

`match <engine> <games> [tc]` and `selfplay <games> [tc]` in the shell play a few games with
the shell's own engine settings (see the book's "How to use this book"). They're for
quick looks, not decisions: they have no SPRT.

## Long tests on the test Mac

The test Mac is an Apple M3 Max on Carlos's network, with 10 performance cores, so 9 games at
a time (from 2026-10-12; the M1 Max ran the tests before). Setting it up once is in
cjus/chessengine#62: Rust, `gh auth login` and `gh auth setup-git`, and a clone of the repo.
Start each test with `nohup`, so that it keeps running when the terminal or SSH session closes:

```sh
git checkout main && git pull
nohup scripts/mac-sprt.sh <PR> > ~/sprt-<PR>.log 2>&1 &
tail -f ~/sprt-<PR>.log
```

```sh
scripts/mac-sprt.sh <PR number> [elo0 elo1] [time control]
```

This builds the PR and `main` in separate worktrees, runs the SPRT with one game per
performance core minus one under `caffeinate`, and posts the result to the PR as a comment,
with any time losses, illegal moves, failures and aborted games. The games are kept in
`target/sprt/pr<PR>-<commit>.pgn` on the test machine. If the match stops without a result
(exit status 2), the comment says **SPRT run failed** and quotes the end of the log instead.

While the match runs, `mac-sprt.sh` also posts the score so far every 2,000 games
(`PROGRESS_EVERY`, 0 for none), as one comment it keeps editing.

Claude follows a running SPRT with `scripts/watch-sprt.sh <PR> [hours] [undecided games]`,
which checks the PR every 10 minutes for the result comment and prints it. If the progress
comment reaches `undecided games` (default 8,000) first, it prints that and exits 3, so that
Claude can propose stopping a run that is taking long to decide. It only reads comments
created after it started, so an earlier run's result on the same PR (a failed run, or the
short time control before the long one) isn't taken for this one's; `SINCE=<ISO time>` or
`SINCE=0` looks further back.

```sh
scripts/fetch-gauntlet-engines.sh [directory]
```

Builds the reference engines (see `docs/PRD.md`, "Testing and strength measurement") from
source, each at the commit CCRL rated, checks that each one plays the match tool's handshake
and a move, and writes two engines files with their CCRL Blitz ratings: `gauntlet.txt`, the
seven gauntlet engines, and `levels.txt`, those and five weaker ones for rating the strength
levels. The default directory is `~/engines/gauntlet`; engines already built there are kept.
It needs Go and rustup's cargo as well as a C compiler (the Rust engines build with this
repository's toolchain), and takes a minute or two.

```sh
scripts/mac-gauntlet.sh <engines file> <issue or PR number> [games per engine] [tc] [git ref]
```

A rating gauntlet: `main` (or the given ref) plays each reference engine in the file (one
`rating path` per line) and the `gauntlet` tool fits cb2026's rating on the reference list's
scale (maximum likelihood with the reference ratings held fixed, see
`crates/tools/src/gauntlet.rs`). Only cb2026 is built at the ref: the gauntlet tool and the
openings come from `main`, so a stage tag, which predates them, works too, with main's match
runner. The result is posted to the issue or PR, with any time losses, illegal moves,
failures and aborted games; a run that breaks posts **Gauntlet run failed** and the end of its
log. Run it at each milestone gate (see `docs/PRD.md`, "Testing and strength measurement"). With
the seven engines at 800 games each, a gauntlet is 5,600 games: about 4 hours on the M3 at
8+0.08.

```sh
scripts/mac-levels.sh <engines file> <issue or PR number> [games per engine] [tc] [git ref]
```

Rates the strength levels: each Skill Level from 0 to 19 plays a gauntlet against the 4 engines
in `levels.txt` nearest its expected rating, and the result is a table of the levels' ratings on
the CCRL Blitz scale, posted to the issue or PR, with a progress comment updated after each
level. The first level is expected where the mapping before the first calibration put it, and
each later one at the gain per level the levels rated so far showed. That only picks the
opponents: each rating comes from the reference engines alone. `LEVELS="0 5 10"` rates some
levels only, and `OPPONENTS=N` changes how many engines each plays. The default time control is
40+0.4: the levels limit nodes, so against opponents given more time they rate lower, 74 to
272 lower at 40+0.4 than at 8+0.08 (#106). At the default 200 games per engine, a level is 800
games, about 1 hour 20 minutes on the M3 at 40+0.4. So the twenty levels would take about 27
hours, and every other level (`LEVELS="0 2 4 6 8 10 12 14 16 18 19"`, the odd ones
interpolated) about 15; at 8+0.08 all twenty took six and a half hours (#103). The games are
saved in `target/levels/`. The ratings feed the `UCI_Elo` mapping in
`crates/engine/src/levels.rs`.

```sh
scripts/mac-smp.sh <PR number> [threads] [time control] [elo0 elo1]
```

From stage 17: plays the PR's build with N threads (default 4) against the same build with
one thread (`match --first-option Threads=N`), one game at a time, and posts the result. Run it
on its own: it needs N + 1 free performance cores. Its bounds default to [0, 25], not [0, 5]:
the question is whether more threads clearly gain, and at one game at a time [0, 5] would take
3–6 hours to confirm a gain of +50 to +100 Elo.

## Bench

From stage 5, `cb2026 bench` searches a fixed set of positions to a fixed depth and prints
the total node count. Every commit message ends with `Bench: <nodes>`. If a change isn't meant
to alter the search, the number must stay the same.

## Rating gauntlets in a cloud session

The `gauntlet` tool runs anywhere:

```sh
cargo run --release -p cb2026-tools --bin gauntlet -- --engine target/release/cb2026 \
    --opponent /path/to/engine-a 2150 --opponent /path/to/engine-b 2400 \
    --games 400 --tc 8+0.08 --concurrency 3
```

`--games` is per opponent. The last line is the rating with its 95% margin.
