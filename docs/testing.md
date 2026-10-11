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
  `nodes=20000` or `depth=4` (machine-independent, good for quick checks).
- **`--sprt elo0 elo1`**: `0 5` for a change meant to gain strength, `-5 0` for a
  simplification that must not lose any. The match stops when the test passes or fails.
- **`--concurrency`**: games at a time. Use the number of cores minus one; in a 4-core cloud
  session, 3.
- **`--openings`**: defaults to `tests/data/openings.epd` (50 positions, each played with both
  colours).
- **Exit status**: 0 if the SPRT passed, 1 if it failed or was inconclusive.

## Long tests on the test Mac

```sh
scripts/mac-sprt.sh <PR number> [elo0 elo1] [time control]
```

This builds the PR and `main` in separate worktrees, runs the SPRT with one game per
performance core minus one under `caffeinate`, and posts the result to the PR as a comment.

```sh
scripts/mac-gauntlet.sh <engines file> <issue or PR number> [games per engine] [tc] [git ref]
```

A rating gauntlet: `main` (or the given ref) plays each reference engine in the file (one
`rating path` per line) and the `gauntlet` tool fits cb2026's rating on the reference list's
scale (maximum likelihood with the reference ratings held fixed, see
`crates/tools/src/gauntlet.rs`). The result is posted to the issue or PR. Run it at each
milestone gate (see `docs/PRD.md`, "Testing and strength measurement").

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
