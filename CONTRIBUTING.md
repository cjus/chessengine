# Contributing to cb2026

Thanks for looking. cb2026 is a chess engine and a learning tool, so a change has to keep it
strong and keep it easy to learn from. Bug reports, book corrections and engine changes are all
welcome.

## Before a pull request

1. Build with the pinned toolchain (`rust-toolchain.toml`): `cargo build --release`.
2. Run `scripts/check.sh`: formatting, clippy, the tests, every perft depth, the docs, licences
   and advisories, and the book. It needs `cargo-deny` and `mdbook`
   (`brew install cargo-deny mdbook`, or `cargo install cargo-deny mdbook`).
3. End the commit message with `Bench: <nodes>`, the node count from `cb2026 bench`.
4. A change to search or evaluation needs a strength test: an SPRT result from the `match` tool
   (`docs/testing.md`). A change that shouldn't play differently says "No functional change"
   and leaves the bench count unchanged.
5. A new or changed technique updates its book chapter (`book/src`) and `docs/elo-ledger.md`.

## The rules the code keeps

- **Chess code is written from scratch.** No chess crates for the board, move generation,
  search, evaluation or book reader. Tooling crates are fine if their licence is permissive.
- **Ideas from any engine, code only from permissive ones.** Name the engine and its licence in
  a comment. Code from GPL engines such as Stockfish can't be used, because cb2026 is
  `MIT OR Apache-2.0`. No other engines' networks or training data.
- **Every technique can be switched off** at runtime, and its module docs say what it does, why
  it works, its pitfalls and where to read more. Every public item has rustdoc.
- **Clarity first.** Optimise only a measured hot spot, and explain the optimisation in a
  comment. `unsafe` only in the marked SIMD and hash-table modules, each with a `// SAFETY:`
  comment.
- **Tunable numbers are named parameters**, never magic numbers in the logic.

`docs/PRD.md` has the full requirements and the decisions behind them.

## Licence

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as in the
README (MIT or Apache-2.0), without any additional terms or conditions.

Security problems go through [SECURITY.md](SECURITY.md), not a public issue.
