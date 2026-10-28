# Apple Silicon microbenchmark experiment (#220)

Run on the Mac being evaluated, with no concurrent NNUE training:

```sh
git rev-parse HEAD
rustc -Vv
sw_vers
sysctl -n machdep.cpu.brand_string
cargo run --release -p cb2026-engine --example arm64_microbench
cargo run --release -p cb2026-cli -- bench
```

Record chip, macOS, power source/mode, temperature/thermal throttling observations, network checksum printed by the example, exact commands, and raw output. The microbenchmark reports nanoseconds per operation for bitboard scan/count, rook+bishop magic lookup, NNUE full refresh/evaluation, and NNUE incremental update/evaluation. Each case has warmup and 11 measurement rounds; the sample median/min/max are descriptive, not statistical confidence intervals.

Repeat with `RUSTFLAGS="-C target-cpu=native"` as a **separate** variant after clearing/isolating build outputs; record all flags. Run at least three independent sessions with the baseline and candidate interleaved to limit thermal drift. Compare `cb2026 bench` nodes and wall-time as well as microbench results. Benchmark loops use `black_box` but remain synthetic: no claims about real search speed or Elo are warranted.

The incremental case reuses a single legal starting-position move, deliberately isolating accumulator arithmetic from move generation. The full refresh uses four representative FENs. Before accepting any optimization, run `scripts/check.sh`, perft, magic-vs-ray and NNUE exact-equivalence tests, then search SPRT where required.

No ANE/Core ML implementation is included. That remains a separately gated experiment after CPU profiling. Do not merge a performance claim without actual Apple Silicon measurements.
