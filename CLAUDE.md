# cb2026 — instructions for Claude

cb2026 (ChessBrain 2026; repo `cjus/chessengine`) is a modern UCI/XBoard chess engine in Rust
that is also a learning tool. Product requirements: `docs/PRD.md`. Owner: Carlos.

**Start here:** `docs/status.md` says what is merged, what is built and waiting on which
branch, what is blocked on Carlos, and how to carry on. Keep it up to date in every PR that
changes the picture.

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
4. Changes to search or evaluation include an SPRT result in the PR (the `match` tool, see
   `docs/testing.md` and `docs/PRD.md` "Testing"), or say "No functional change" with an
   unchanged bench.
5. Book chapter and `docs/elo-ledger.md` updated when a technique is added or changed.
6. Work is on a `claude/<topic>` branch from the latest `main`, with a PR to `main` and green CI.
   `main` is protected (ruleset `main: PR + CI`): changes land only through a PR whose CI check
   (`CI`: fmt, clippy, tests, perft, docs, licences and advisories, and the book, in one job)
   passed. Never try to get around it.
   **Claude may merge its own PRs** (granted by Carlos 2026-10-10) once `CI` is green on the
   latest commit, the PR merges cleanly and no review comment is unanswered.
   **Spend few Actions minutes** (they ran out in October 2026, cjus/chessgui#73). Open a PR as a
   draft (`gh pr create --draft`) and mark it ready (`gh pr ready`) when its work is done: CI
   checks only PRs that are ready, and again on each push after that, so push a PR's commits
   together. A PR that changes only `docs/` or Markdown outside the book passes in seconds.
   `.github/workflows/ci.yml` says what else saves minutes. After a
   stage's last PR merges, it gets a `stage-NN-<name>` tag on `main`. Cloud sessions can't push
   tags (the git proxy refuses them), so add the tag command to the open `needs-carlos` issue
   "Push stage tags" (cjus/chessengine#17, or a new one if that is closed).
7. If the change affects `docs/PRD.md` scope or decisions, update it in the same PR.

## What Claude runs and what Carlos runs

Claude may run everything in the repo: cargo, perft, bench, fuzzing, short SPRT matches.
Long SPRTs, gauntlets and checks in real GUIs are Carlos's, on his Apple Silicon Mac. Claude
writes the scripts (`scripts/mac-sprt.sh <PR>`, `scripts/mac-gauntlet.sh`), which post results to
the PR as a comment; for GUI checks, hand Carlos the exact steps, the expected result and what to
paste back.

## Items for Carlos go in GitHub issues

Anything Carlos needs to do, decide or answer (a setting, a check on the Mac, a scope question)
is a GitHub issue assigned to `cjus` with the `needs-carlos` label, not only a line in chat or
a PR description. Each issue says why it matters, the exact steps, the expected result, and
"Done when". Before filing, check the open `needs-carlos` issues for a duplicate. When Carlos
answers in a comment, act on it (update the PRD, open the PR) and close the issue; when he
closes it himself, pick up any follow-up it names. His list:
https://github.com/cjus/chessengine/issues?q=is%3Aopen+label%3Aneeds-carlos
