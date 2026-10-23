# cb2026 — instructions for Claude

cb2026 (ChessBrain 2026; repo `cjus/chessengine`) is a modern UCI/XBoard chess engine in Rust
that is also a learning tool. Product requirements: `docs/PRD.md`. Owner: Carlos.

**Start here:** `docs/status.md` says what is merged, what is built and waiting on which
branch, what is blocked on Carlos, and how to carry on. Keep it up to date in every PR that
changes the picture.

## Architecture rules

1. **Chess logic lives in library crates with no I/O** (`crates/chess`, `crates/engine`).
   `crates/protocol`, `crates/cnp` (the Chess Node Protocol), `crates/shell` and `crates/cli`
   are thin adapters over the Engine API.
2. **Chess code is written from scratch.** No chess crates for the board, move generation,
   search, evaluation or book reader. Tooling crates are fine if their licence is permissive
   (`cargo deny check licenses`): cb2026 is `MIT OR Apache-2.0`. The one exception is
   `shakmaty-syzygy` (GPL) for tablebase probing, used only behind the `Tablebase` trait and
   the `syzygy` feature (PRD "Tablebase probing", "License and IP").
3. **Every technique is switchable** at runtime and documented: module docs say what it does,
   why it works, its pitfalls and links (Chess Programming Wiki, papers). Every public item has
   rustdoc (`#![deny(missing_docs)]`).
4. **Clarity first.** Optimise only measured hot spots, and explain the optimisation in a comment.
5. **`unsafe` only in marked modules** (SIMD, lock-free hash table) with `// SAFETY:` comments;
   every other crate has `#![forbid(unsafe_code)]`.
6. **Credit sources.** When an idea or code comes from a specific engine, name it and its licence
   in a comment. Ideas may come from any engine, GPL ones included; **code only from permissive
   sources** (MIT, Apache-2.0, BSD, ISC, Zlib, public domain), never from GPL engines such as
   Stockfish, because cb2026 is `MIT OR Apache-2.0`. No other engines' nets or training data.
7. Tunable numbers are named parameters, never magic numbers in logic.

## Definition of done for a change

1. `scripts/check.sh` passes on the PR's latest commit: fmt, clippy, tests, every perft depth,
   docs, licences and advisories, and the book (what CI checked on PRs until 2026-10-22).
2. Perft suites still match (every depth, in `scripts/check.sh` and the nightly run).
3. The commit message ends with `Bench: <nodes>` (from `cb2026 bench`).
4. Changes to search or evaluation include an SPRT result in the PR (the `match` tool, see
   `docs/testing.md` and `docs/PRD.md` "Testing"), or say "No functional change" with an
   unchanged bench.
5. Book chapter and `docs/elo-ledger.md` updated when a technique is added or changed.
6. Work is on a `claude/<topic>` branch from the latest `main`, with a PR to `main`.
   `main` is protected (ruleset `main: PR`): changes land only through a PR. Never try to get
   around it. **GitHub doesn't check PRs** (since 2026-10-22: Actions minutes ran out in
   October 2026, cjus/chessgui#73), so `scripts/check.sh` is the check: run it before merging.
   CI checks `main` every night, only if it changed; if a nightly run fails, fixing it comes
   first. To have GitHub check a branch anyway: `gh workflow run ci.yml --ref <branch>`.
   **Claude may merge its own PRs** (granted by Carlos 2026-10-10) once `scripts/check.sh`
   passes on the latest commit, the PR merges cleanly and no review comment is unanswered.
   Open a PR as a draft (`gh pr create --draft`) while it waits on something (an SPRT, Carlos)
   and mark it ready (`gh pr ready`) when its work is done. After a
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

## Building on the farm (pnxfarm)

Release builds of pushed commits run on the farm's Macs, not this one, through pnxfarm
(PNXStudios/pnxfarm; its guide for repositories is `docs/using-pnxfarm.md` there). For now the
farm has one worker, the M3. Use it to get a branch's binaries instead of
`cargo build --release` here. The recipe is `pnxfarm.toml`.

- Push the commit, then run `pnxfarm build --wait` as a background command. The last line it
  prints is the job, as JSON; when it prints none, the reason is the last line of its error
  output.
- The binaries (`cb2026` and the tools) land in `target/pnxfarm/release/`, never
  `target/release/`, each with a `<binary>.json` saying which commit and Mac built it.
- Its exit code says what to do next: 0, built and installed; 1, the build failed, so fix the
  errors it printed; 2, the farm couldn't run it (unreachable, or no worker will take it soon:
  every Mac is paused, say), so say so and build here only if asked; 3, timed out, and
  `pnxfarm wait <job>` carries on; 4, refused, so do what the message says.
- `pnxfarm status`, `pnxfarm log <job>` and `pnxfarm workers` show what the farm is doing.
- **The `Bench:` line of a commit not yet made** still needs a build here, until pnxfarm builds
  uncommitted changes (its v2). For a pushed commit, `target/pnxfarm/release/cb2026 bench`
  gives the same number.
- The long-run scripts (`scripts/mac-*.sh`) pause the M3's worker for their run, after the build
  it has in hand, and resume it when they end (`scripts/pnxfarm-pause.sh`). While the M3 is the
  only worker, a build asked for during such a run waits for it: `pnxfarm build --wait` exits 2
  after two minutes, saying the M3 is paused, and the job stays queued. Say so;
  `pnxfarm wait <job>` installs it once the run ends.
- **After the M3 restarts,** its worker is offline until Carlos logs in to it (FileVault rules
  out automatic login), and `pnxfarm build --wait` exits 2 saying no workers are online. Tell
  him.

## Items for Carlos go in GitHub issues

Anything Carlos needs to do, decide or answer (a setting, a check on the Mac, a scope question)
is a GitHub issue assigned to `cjus` with the `needs-carlos` label, not only a line in chat or
a PR description. Each issue says why it matters, the exact steps, the expected result, and
"Done when". Before filing, check the open `needs-carlos` issues for a duplicate. When Carlos
answers in a comment, act on it (update the PRD, open the PR) and close the issue; when he
closes it himself, pick up any follow-up it names. His list:
https://github.com/cjus/chessengine/issues?q=is%3Aopen+label%3Aneeds-carlos
