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
- **Exit status**: 0 if the SPRT passed, 1 if it failed or was inconclusive.

## Robustness runs

The PRD asks for zero illegal moves and crashes over 10,000+ games per milestone, and zero time
losses at 1+0.01, 10+0.1 and 40/60 repeating, with the default `Move Overhead`. The `match`
tool's last lines count the games that ended badly, per engine:

```text
Problems for target/release/cb2026: 0 time losses, 0 illegal moves, 0 failures
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
with any time losses, illegal moves and failures. The games are kept in
`target/sprt/pr<PR>-<commit>.pgn` on the test machine.

Claude follows a running SPRT with `scripts/watch-sprt.sh <PR> [hours]`, which checks the PR
every 10 minutes for the result comment and prints it.

```sh
scripts/mac-gauntlet.sh <engines file> <issue or PR number> [games per engine] [tc] [git ref]
```

A rating gauntlet: `main` (or the given ref) plays each reference engine in the file (one
`rating path` per line) and the `gauntlet` tool fits cb2026's rating on the reference list's
scale (maximum likelihood with the reference ratings held fixed, see
`crates/tools/src/gauntlet.rs`). The result is posted to the issue or PR. Run it at each
milestone gate (see `docs/PRD.md`, "Testing and strength measurement").

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
