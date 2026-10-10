# cb2026 — instructions for Claude

cb2026 (ChessBrain 2026; repo `cjus/chessengine`) is a modern UCI/XBoard chess engine in Rust
that is also a learning tool. Product requirements: `docs/PRD.md`. Owner: Carlos.

## Architecture rules

1. **Chess logic lives in library crates with no I/O** (`crates/chess`, `crates/engine`).
   `crates/protocol`, `crates/shell` and `crates/cli` are thin adapters over the Engine API.
2. **Chess code is written from scratch.** No chess crates for the board, move generation,
   search, evaluation or book reader. Tooling crates are fine if their licence is
   GPL-3.0-compatible (`cargo deny check licenses`). The one exception is `shakmaty-syzygy`
   for tablebase probing, used only behind the `Tablebase` trait (PRD "Tablebase probing").
3. **Every technique is switchable** at runtime and documented: module docs say what it does,
   why it works, its pitfalls and links (Chess Programming Wiki, papers). Every public item has
   rustdoc (`#![deny(missing_docs)]`).
4. **Clarity first.** Optimise only measured hot spots, and explain the optimisation in a comment.
5. **`unsafe` only in marked modules** (SIMD, lock-free hash table) with `// SAFETY:` comments;
   every other crate has `#![forbid(unsafe_code)]`.
6. **Credit sources.** When an idea or code comes from a specific engine, name it and its licence
   in a comment. Only GPL-3.0-compatible code. No other engines' nets or training data.
7. Tunable numbers are named parameters, never magic numbers in logic.

## Definition of done for a change

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass.
2. Perft suites still match (quick depth in CI; full depth before a stage tag).
3. The commit message ends with `Bench: <nodes>` (from `cb2026 bench`).
4. Changes to search or evaluation include an SPRT result in the PR (fastchess, see
   `docs/PRD.md` "Testing"), or say "No functional change" with an unchanged bench.
5. Book chapter and `docs/elo-ledger.md` updated when a technique is added or changed.
6. Work is on a `claude/<topic>` branch from the latest `main`, with a PR to `main` and green CI.
7. If the change affects `docs/PRD.md` scope or decisions, update it in the same PR.

## What Claude runs and what Carlos runs

Claude may run everything in the repo: cargo, perft, bench, fuzzing, short SPRT matches.
Long SPRTs, gauntlets and checks in real GUIs are Carlos's, on his Apple Silicon Mac. Claude
writes the scripts (`scripts/mac-sprt.sh <PR>`, `scripts/mac-gauntlet.sh`), which post results to
the PR as a comment; for GUI checks, hand Carlos the exact steps, the expected result and what to
paste back.
