# Chess Engine in Rust — Product Requirements

Working name: **chessengine** (the final engine name is an open question, see the end).
Status: draft for review. Last updated: 2026-10-10.

## Overview

We will build a modern chess engine in Rust that does two jobs at once:

1. **A real engine.** A UCI backend that plays at master strength (2400–2700 on the CCRL scale)
   and works in the open-source and commercial chess GUIs people already use: Cute Chess, Arena,
   Banksia, En Croissant, ChessBase/Fritz, Hiarcs Chess Explorer, Shredder and others.
2. **A learning tool.** Every technique is explained in the code, in a companion book and in an
   interactive shell, so a reader can follow the engine from an empty board to master strength and
   see why each step makes it stronger.

The engine is a backend, like Stockfish or Ethereal: it has no graphical interface of its own.
Instead it ships with a **rich command-line shell** where you can play complete games, watch the
engine think, set its level, and look inside it: bitboards, attack maps, hash keys, evaluation terms
and the search tree.

**Goals**

- Master strength: at least 2400 CCRL Blitz for v1.0, aiming for 2600–2700, using a classical
  alpha-beta search and a tuned hand-crafted evaluation.
- Works everywhere engines are used: full UCI, plus XBoard/CECP, Chess960 and a Lichess bot setup.
- Teaches: each technique has rustdoc docs, a book chapter, a git tag where it can be run on its own,
  an on/off switch, and a measured Elo value.
- Robust: no illegal moves, no crashes, no losses on time in GUIs or tournaments.

**Non-goals for v1**

- No graphical interface. GUIs and the shell are the front ends.
- No Monte Carlo tree search or large GPU neural networks (the AlphaZero/Leela family).
- No chess variants beyond Chess960.
- No code, nets or training data copied from other engines unless the license allows it and we
  credit it (see "License and IP").

## Background: what "modern" means here

Short version for readers of the book (each point gets its own chapter):

- **Search is still alpha-beta.** Every top engine searches with alpha-beta (negamax). What changed
  since the 1990s is how much of the tree is cut away: iterative deepening, principal variation
  search, a large transposition table, null-move pruning, late move reductions, futility and SEE
  pruning, singular extensions, and history-based move ordering. A modern engine searches a tiny,
  well-chosen part of the tree very deep.
- **Evaluation comes in two styles.** A *hand-crafted evaluation* (HCE) adds up terms a person
  designed (material, piece placement, pawn structure, king safety, mobility), with weights tuned
  automatically on millions of positions ("Texel tuning"). *NNUE*, used by almost every top engine
  since 2020, replaces those terms with a small neural network that runs fast on an ordinary CPU.
  NNUE is worth roughly 200–400 Elo, but needs a data and training pipeline.
- **Decision (2026-10-10):** reach the v1 target with alpha-beta and a tuned HCE, where every
  term can be explained. NNUE is an optional stretch milestone (M6) written as a teaching chapter;
  the HCE stays available as a switchable mode.

## Users and use cases

| User | What they do | What they need |
| --- | --- | --- |
| Carlos (owner) | Plays and analyses in the shell; runs the engine in GUIs on the Mac | A fun, readable shell; the engine to behave in every GUI he uses |
| Learner | Reads the book, checks out a milestone tag, runs it, changes it, measures it | Clear code, chapters linked to code, techniques that can be switched off, an Elo record |
| Chess player | Loads the engine into a GUI to analyse or play at a chosen level | Standard UCI options, a sensible level/Elo limit, stable analysis |
| Engine tester | Runs it in a tournament manager (fastchess, Cute Chess) or a rating list | Correct protocol, honest time handling, Chess960, reproducible `bench` |
| Lichess player | Plays the bot online | A Lichess BOT account running through lichess-bot |

## Platforms

| Platform | Role | Builds |
| --- | --- | --- |
| macOS, Apple Silicon | Primary (Carlos's machines) | `aarch64-apple-darwin`, NEON |
| Linux x86-64 | Claude cloud sessions, CI, testing machines | baseline (x86-64-v2, POPCNT) and AVX2/BMI2 (x86-64-v3) |
| Windows x86-64 | ChessBase/Fritz and most commercial GUIs | baseline and AVX2/BMI2 `.exe` |
| Linux aarch64 (Raspberry Pi and similar) | Could-have | `aarch64-unknown-linux-gnu` |

One self-contained binary per platform, no installer and no runtime dependencies, so a GUI can
simply point at it. Built with stable Rust (pinned in `rust-toolchain.toml`, Rust 2024 edition).
CPU features are chosen at compile time, one binary per target level, which is the usual practice
for engines; the release notes say which binary to pick.

## Architecture

A Cargo workspace. The rule from deepbore applies here too: **all chess logic lives in library
crates with no I/O**, and the protocols and the shell are thin adapters on top. That keeps the
logic testable and lets UCI, XBoard and the shell share exactly the same engine.

```
crates/
  chess/      board, bitboards, magic tables, move generation, make/unmake,
              Zobrist hashing, FEN, SAN/UCI move notation, perft. No dependencies.
  engine/     search, evaluation (HCE; NNUE later), transposition table,
              time management, threads (Lazy SMP), tablebases, opening book,
              strength limiting. Exposes an Engine API with an info callback.
  protocol/   UCI and XBoard/CECP adapters over the Engine API.
  shell/      the interactive REPL: commands, board views, live thinking display.
  tools/      tuner (Texel), self-play data generation, bench, test helpers.
  nnue/       (M6) NNUE inference and trainer.
  cli/        the single `chessengine` binary: picks UCI, XBoard or shell mode.
book/         mdBook: "Building a chess engine in Rust".
docs/         PRD, Elo ledger, testing guide, GUI compatibility matrix.
tests/data/   perft suites, EPD test suites, bench positions.
scripts/      SPRT and gauntlet runners (fastchess), release helpers.
```

**One binary, three modes.** GUIs launch the engine and send `uci` (or `xboard`) as their first
line, so the binary chooses its mode from that first line: `uci` → UCI, `xboard` → CECP, anything
else on an interactive terminal → the shell. `--uci`, `--xboard` and `--shell` force a mode, and
`chessengine bench` runs the benchmark (see Testing).

**Engine API.** `Engine::search(position, limits, options, on_info)` returns a best move (and a
ponder move), and calls `on_info` with depth, score, PV, nodes, speed and other statistics while it
searches. Search runs on its own threads, so `stop`, `isready` and `ponderhit` are always answered
immediately.

### Board and move generation

- **Bitboards** (one 64-bit set per piece type and colour) plus a mailbox array for quick
  "what's on this square" lookups.
- **Magic bitboards** for sliding pieces; an optional PEXT build for CPUs with fast BMI2 (not AMD
  Zen 1/2, where PEXT is slow). The book first builds sliding attacks the slow, obvious way
  (ray loops) and then replaces them with magics, measuring the speedup.
- **Legal move generation** using check and pin masks, so the search never sees illegal moves.
- **Make/unmake** with an undo stack; incremental Zobrist hash, pawn hash and material key.
- **Chess960 castling** handled in the same code as standard castling (king-takes-rook encoding
  inside the engine, converted at the protocol boundary).
- **Verified by perft:** the standard perft suite (start position, "Kiwipete" and the other usual
  test positions) and a Chess960 perft suite must match their known node counts exactly. Target
  speed: at least 100 million nodes/s single-threaded with bulk counting on an Apple M1.

### Search

Each technique below is a separate function or clearly marked block, has its own on/off switch
(see "Learning tool"), and gets its Elo gain measured and recorded when it's added.

| Area | Techniques |
| --- | --- |
| Core | Negamax alpha-beta, iterative deepening, principal variation search, aspiration windows, quiescence search |
| Transposition table | Zobrist keys, bucketed entries with age-based replacement, mate-score adjustment, prefetch, lock-free sharing between threads (checked keys) |
| Move ordering | TT move, captures by MVV-LVA and SEE, killer moves, history heuristic, countermoves, continuation history, staged move generation |
| Pruning and reductions | Null-move pruning, reverse futility pruning, futility pruning, late move pruning, late move reductions, razoring, SEE pruning in main and quiescence search, delta pruning, internal iterative reductions, mate distance pruning |
| Extensions | Check extension; singular extensions (should-have) |
| Draws | Repetition detection (with the search path and game history), fifty-move rule, insufficient material |
| Parallel | Lazy SMP: up to 256 threads sharing one transposition table |
| Time management | Hard and soft limits from clock, increment and moves-to-go; extra time when the best move changes or the score drops; a `Move Overhead` safety margin |
| Limits | depth, nodes, movetime, mate in N, infinite, searchmoves, ponder, MultiPV |

### Evaluation (hand-crafted)

- **Tapered evaluation:** every term has a middlegame and an endgame value, blended by game phase.
- **Terms:** material, piece-square tables, mobility, pawn structure (passed, isolated, doubled,
  backward, connected, candidate pawns; cached in a pawn hash table), passed-pawn race and king
  distance, king safety (attacks on the king zone, pawn shield and storm, open files), bishop pair,
  rooks on open and semi-open files and the seventh rank, outposts, threats (pieces attacked by
  pawns and by lesser pieces), tempo, and endgame scaling (opposite-coloured bishops, known drawish
  material, driving the king to the edge in won endgames).
- **Texel tuning:** all weights tuned by gradient descent on positions labelled with game results.
  The data comes from the engine's own self-play games (see "License and IP" for public datasets).
- **Explainable:** evaluation is written once and can record a *trace*: a per-term, per-side
  breakdown for the shell's `eval` command and for tuning, at no cost when tracing is off.

### Extras

| Feature | Priority | Notes |
| --- | --- | --- |
| Syzygy endgame tablebases | Must | WDL probes in search, DTZ at the root; `SyzygyPath`, `SyzygyProbeDepth`, `SyzygyProbeLimit`. Probing code is a dependency decision, see Open questions |
| Polyglot opening book | Must | Our own `.bin` reader; `OwnBook` and `BookFile` options; off by default so testing isn't affected |
| MultiPV | Must | Up to 256 lines |
| Strength limiting | Must | `UCI_LimitStrength`, `UCI_Elo`, `Skill Level` (see "Levels") |
| Win/draw/loss output | Should | `UCI_ShowWDL`, from a model fitted to our self-play results |
| Pondering | Must | `Ponder` option, `go ponder` / `ponderhit` |
| Bench | Must | Fixed list of positions searched to a fixed depth; prints total nodes and speed. The node count is the search's fingerprint |

## Protocols and GUI compatibility

### UCI (must)

Full UCI as GUIs actually use it, including `debug`, `setoption`, `ucinewgame`, `position fen ...
moves ...`, every `go` parameter, `stop`, `ponderhit`, `quit`, and `info` lines with `depth`,
`seldepth`, `multipv`, `score cp|mate`, `lowerbound`/`upperbound`, `wdl`, `nodes`, `nps`,
`hashfull`, `tbhits`, `time`, `pv`, `currmove` and `string`.

| Option | Type | Default | Range |
| --- | --- | --- | --- |
| Hash | spin (MB) | 16 | 1–65536 |
| Threads | spin | 1 | 1–256 |
| MultiPV | spin | 1 | 1–256 |
| Ponder | check | false | |
| Move Overhead | spin (ms) | 30 | 0–5000 |
| UCI_Chess960 | check | false | |
| UCI_ShowWDL | check | false | |
| UCI_LimitStrength | check | false | |
| UCI_Elo | spin | 2400 | 1000–2400 (final range after calibration) |
| Skill Level | spin | 20 | 0–20 |
| SyzygyPath | string | `<empty>` | |
| SyzygyProbeDepth | spin | 1 | 1–100 |
| SyzygyProbeLimit | spin | 7 | 0–7 |
| OwnBook | check | false | |
| BookFile | string | `<empty>` | |
| Clear Hash | button | | |
| EvalFile | string | built-in net | M6 only |

Technique switches and tuning parameters are not shown to GUIs by default; they appear only in a
developer build or when the engine is started with `--dev`, so GUI option lists stay clean.

### XBoard/CECP (must)

Protocol version 2 through the same Engine API: `protover 2` features (`setboard`, `usermove`,
`ping`, `san=0`, `analyze`, `colors=0`, `memory`, `smp`, `egt="syzygy"`, `variants="normal,fischerandom"`),
`new`, `force`, `go`, `level`, `st`, `sd`, `time`/`otim`, `post`/`nopost`, `analyze`, `undo`,
`remove`, `hard`/`easy` (pondering), `result`, `quit`. Thinking output in the standard
`ply score time nodes pv` format.

### Chess960 (must)

Through `UCI_Chess960` and XBoard `variant fischerandom`, with Shredder-FEN and X-FEN castling
rights both accepted. Chess960 perft and Chess960 test matches are part of the test suite.

### Lichess bot (must)

A documented setup (`docs/lichess.md`) for running the engine as a Lichess BOT account with
[lichess-bot](https://github.com/lichess-bot-devs/lichess-bot): engine config, recommended options,
and which time controls to accept. lichess-bot is a separate project; we don't bundle it.

### Compatibility matrix

A checklist in `docs/gui-compat.md`, re-run at each release, covering: load, options dialog,
analysis with MultiPV, play a timed game, Chess960 game, pondering, and adding/removing the engine.

| GUI / tool | Platform | License | Who checks |
| --- | --- | --- | --- |
| fastchess, cutechess-cli | Linux/macOS | Open source | Claude (scripted, every release) |
| Cute Chess | macOS/Linux/Windows | Open source | Carlos |
| En Croissant | macOS/Windows/Linux | Open source | Carlos |
| Arena, Banksia | Windows (Banksia also macOS/Linux) | Free | Carlos |
| XBoard | macOS/Linux | Open source | Carlos (CECP mode) |
| ChessBase / Fritz | Windows | Commercial | Carlos (if available) |
| Hiarcs Chess Explorer | macOS/Windows | Commercial | Carlos (if available) |
| Shredder GUI | macOS/Windows | Commercial | Carlos (if available) |
| lichess-bot | Linux/macOS | Open source | Claude sets it up; Carlos runs it |

Claude can't operate GUIs, so it also maintains a **protocol conformance test**: a script that
plays scripted UCI and XBoard sessions against the binary and checks every response, including
awkward GUI habits (options sent mid-search, `position` without `ucinewgame`, very short clocks,
`stop` straight after `go`, and Windows line endings).

## The interactive shell

Started with `chessengine` in a terminal (or `chessengine --shell`). It's a line-based REPL
with history, tab completion and colour. It works over SSH and in any terminal, falls back to ASCII
when Unicode or colour isn't available, and respects `NO_COLOR`. `source <file>` runs a script of
commands, which also makes the shell testable.

### Board display

```
   ┌────────────────────────┐
 8 │ ♜  ♞  ♝  ♛  ♚  ♝  ·  ♜ │   White to move · move 3
 7 │ ♟  ♟  ♟  ♟  ·  ♟  ♟  ♟ │   Castling: KQkq   En passant: -
 6 │ ·  ·  ·  ·  ·  ♞  ·  · │   50-move: 2/100
 5 │ ·  ·  ·  ·  ♟  ·  ·  · │   Eval: +0.31 (HCE, static)
 4 │ ·  ·  ·  ·  ♙  ·  ·  · │   Hash: 0x…  (Zobrist key)
 3 │ ·  ·  ·  ·  ·  ♘  ·  · │   Last: ...Nf6
 2 │ ♙  ♙  ♙  ♙  ·  ♙  ♙  ♙ │
 1 │ ♖  ♘  ♗  ♕  ♔  ♗  ·  ♖ │
   └────────────────────────┘
     a  b  c  d  e  f  g  h
FEN: rnbqkb1r/pppp1ppp/5n2/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3
```

Coloured squares in a colour terminal, last move and checks highlighted, `flip` to view from either
side, `board ascii|unicode` to switch styles.

### Commands (first pass; final names decided during M1)

| Group | Commands |
| --- | --- |
| Position | `new`, `fen <FEN>`, `startpos`, `new960 [n]`, `moves`, typing a move (`e4`, `Nf3`, `e2e4`), `undo`, `redo`, `flip`, `show` |
| Play | `play white\|black\|both\|none` (which sides the engine plays), `time 5+3` (clocks), `level <0-20>`, `elo <n>`, `hint`, `resign`, `draw`, `pgn save\|load <file>` |
| Thinking | `go [depth n\|nodes n\|movetime ms]`, `analyze` (infinite, live display), `stop`, `multipv <n>`, `think on\|off` (show thinking while the engine plays), `threads <n>`, `hash <MB>` |
| Look inside | `bb <piece\|occupancy\|attacks\|pins\|checkers>`: bitboard as an 8×8 grid plus hex; `attacks <square>`; `magic <square>` (mask, magic number, index, result); `zobrist` (hash broken into its parts); `see <move>`; `legal` (all legal moves with SAN and their move-ordering scores); `perft <n>`, `divide <n>` |
| Explain | `eval` (term-by-term breakdown, middlegame/endgame/blended, both sides); `eval diff <move>` (what a move changes); `explain <move>` (searches the move and compares it with the best move, showing the refutation); `tree <depth>` (prints or exports the search tree, with pruned and reduced nodes marked); `stats` (TT hit rate, first-move cutoff rate, nodes saved by each pruning technique, LMR re-searches) |
| Techniques | `techniques` (lists them, on/off); `set <technique> on\|off`; `ab <depth>` (searches with and without one technique and compares nodes, time and move) |
| Tools | `bench`, `selfplay <n> [tc]`, `match <engine-path> <n> [tc]` (a small match against another UCI engine, should-have), `uci` / `xboard` (switches the session to that protocol), `help [command]`, `quit` |

### Engine thinking display

While the engine searches, the shell shows one updating line per depth: depth/seldepth, score
(pawns or mate, plus W/D/L when on), nodes, speed, time, hash usage, tablebase hits, and the PV in
SAN. With MultiPV, every line is shown. A one-line summary stays on screen after each move, and
every game can be saved as PGN with the engine's score and depth as comments (`{+0.35/18}`).

### Playing complete games

Human vs engine (either colour), engine vs engine (self-play) or human vs human. Supports clocks
with increment, all draw rules (threefold repetition, fifty moves, insufficient material,
stalemate), the engine offering and accepting draws and resigning in hopeless positions
(configurable, off for testing), and saving and loading PGN.

## Levels and strength limiting

- **`Skill Level` 0–20** and **`UCI_LimitStrength` + `UCI_Elo`**, both mapped to the same
  mechanism. 20 / no limit means full strength.
- **How it weakens:** a node limit per move, plus choosing among the top MultiPV moves with
  controlled randomness, so it makes believable mistakes instead of blundering pieces at random.
  The book explains why "search less" alone doesn't produce human-like play.
- **Calibrated, not guessed:** each level is measured in matches against reference engines with
  known CCRL ratings, and the `UCI_Elo` → settings table is fitted to those results. Elo is on the
  CCRL scale; the docs say plainly that this isn't the same as a FIDE or Lichess rating.
- The shell's `level` and `elo` commands set the same options.

## Learning tool

**Decided 2026-10-10:** all four learning features are in scope.

1. **Documented code.** Every public item has rustdoc (enforced with `#![deny(missing_docs)]`),
   and every technique's module starts with what it does, why it works, its known pitfalls, and
   links to the Chess Programming Wiki and original papers. Comments explain *why*, not *what*.
   Where speed and clarity conflict, clarity wins unless the speedup is measurable; optimised hot
   paths explain themselves in comments.
2. **Companion book (mdBook)** in `book/`: "Building a chess engine in Rust", one chapter per
   technique. Each chapter covers the idea, the code (included from the real source using mdBook
   anchors so it can't drift out of date), how to test it, the measured Elo gain, and further
   reading. It's built and checked in CI; publishing it on GitHub Pages waits until the repo is
   public.
3. **Milestone tags.** Git tags `stage-NN-<name>` mark each stage below. Every tag builds, passes
   its tests, plays legal chess through UCI, and matches its book chapter, so a learner can run
   `git checkout stage-07-quiescence` and play against that version.
4. **Technique switches.** Every search technique and evaluation term group can be turned off at
   runtime (shell `set`, or `--dev` UCI options), so learners can measure each one themselves. An
   **Elo ledger** (`docs/elo-ledger.md`) records the measured gain for every stage and technique,
   with the test conditions.

### Stages (tags and book chapters)

Rough Elo figures are what engines typically reach at each stage; ours are measured and go in the ledger.

| Tag | Stage | Rough strength |
| --- | --- | --- |
| stage-01-board | Bitboards, squares, pieces, FEN, printing the board | — |
| stage-02-movegen | Move generation (rays first), legality, castling, en passant, promotions, perft | — |
| stage-03-magics | Magic bitboards; perft speed | — |
| stage-04-uci | Zobrist hashing, UCI, a random mover playable in a GUI | — |
| stage-05-alphabeta | Material evaluation, negamax, alpha-beta | ~1200 |
| stage-06-iterative | Iterative deepening, time management | |
| stage-07-quiescence | Quiescence search, MVV-LVA | ~1600 |
| stage-08-tt | Transposition table | |
| stage-09-pst | Piece-square tables, tapered evaluation | ~1900 |
| stage-10-ordering | Killers, history, countermoves, staged move generation | |
| stage-11-pvs | Principal variation search, aspiration windows | |
| stage-12-nullmove | Null-move pruning | |
| stage-13-lmr | Late move reductions and late move pruning | ~2200 |
| stage-14-pruning | Reverse futility, futility, razoring, SEE pruning | |
| stage-15-hce | Full hand-crafted evaluation | |
| stage-16-tuning | Texel tuning on self-play data | ~2500 |
| stage-17-smp | Lazy SMP | |
| stage-18-tablebases | Syzygy, Polyglot book, Chess960 | |
| stage-19-levels | Strength limiting and calibration | |
| stage-20-xboard | XBoard/CECP, Lichess bot | v1.0 |
| stage-21-nnue | NNUE (M6, stretch) | ~2800+ |

The shell grows along with the stages: board display and play from stage 4, `perft` and the
bitboard views from stage 2–3, `eval` and `explain` from stage 9 on.

## Testing and strength measurement

| Layer | What it proves | Runs where |
| --- | --- | --- |
| Unit and property tests | make/unmake restores the position; incremental hash equals a full recompute; FEN round-trips; SAN parsing | CI on every PR; Claude in session |
| Perft suites | Move generation is exactly right (standard and Chess960) | CI (quick depth); full depth before tags |
| Fuzzing (`cargo fuzz`) | The FEN, SAN, UCI and XBoard parsers never panic on bad input | Locally and nightly |
| Protocol conformance | Correct UCI/XBoard behaviour, including GUI quirks | CI |
| Bench | Search didn't change by accident: same node count = same search | CI; recorded in every commit message |
| Tactical suites (EPD) | Sanity check: e.g. most WAC positions solved in 1 s | Before tags |
| SPRT self-play | Did this change gain Elo? | Claude's session (short), Carlos's Mac or a test box (long) |
| Gauntlet vs reference engines | Where we are on the CCRL scale | Each milestone gate |

**SPRT rules.** Every change that affects search or evaluation is tested with fastchess using a
sequential probability ratio test, against the current `main`:
- gainers: `elo0=0, elo1=5` at short time control (8+0.08 s), and at long time control (40+0.4 s) for
  pruning and time-management changes;
- simplifications and refactors: non-regression bounds `[-5, 0]`;
- balanced opening book, games played in pairs with colours swapped.

Every commit message carries `Bench: <nodes>`. A PR changing search or evaluation includes its
SPRT result (Elo ± error, games played, LLR) or says "No functional change" with an unchanged bench.

**Rating.** At each gate, a gauntlet of several thousand games against 4–6 open-source engines
whose CCRL Blitz ratings bracket our target (candidates: versions of Stash, Rustic, Blunder,
Fruit 2.1, Ethereal and Berserk versions; final list chosen in M0), with Ordo to estimate our
rating. These engines are downloaded by a script for testing only and are never included in the repo.

**Robustness requirements**
- Zero illegal moves and zero crashes over 10,000+ games of testing per milestone.
- Zero time losses at 1+0.01 s, 10+0.1 s and 40/60 s repeating time controls, with default `Move
  Overhead`.
- `stop` answered within 10 ms; `isready` answered while searching.
- Searches are deterministic single-threaded for a given node limit, so bugs are reproducible.
- `unsafe` code only in a few marked modules (SIMD, lock-free hash table), each with a `// SAFETY:`
  comment; every other crate uses `#![forbid(unsafe_code)]`.

## Building it with Claude

Most code is written by Claude in cloud sessions attached to the repo (Linux, 4 cores, Rust stable).
Unlike deepbore, there are no license limits on tooling: Claude can build, test, run perft, bench
and short SPRT matches itself. What Claude can't do is look at GUIs or run long tests on the Mac.

| Task | Claude (cloud) | Carlos |
| --- | --- | --- |
| Write code, tests, book chapters | Yes | Reviews PRs |
| `cargo test`, clippy, fmt, perft, bench, fuzzing | Yes | Optional |
| Short SPRT (a few thousand STC games on 4 cores) | Yes | |
| Long SPRT, LTC, gauntlets | Prepares the scripts | Runs them on the Mac or a test box, pastes results |
| GUI compatibility checks | Protocol conformance tests | Checks real GUIs, especially commercial ones |
| Playing and judging the shell UX | No | Yes |

**Workflow:** `claude/<topic>` branches from the latest `main`, one PR each; CI (fmt, clippy,
tests, quick perft, bench, conformance, book build) must be green; the PR states bench and SPRT
results. Each stage ends with a tag, a book chapter and an Elo ledger entry.

**CI cost.** GitHub Actions minutes are limited on private repos, and macOS and Windows minutes
have historically cost 10× and 2× Linux minutes. So PRs run on Linux; macOS and Windows builds run nightly and on
release tags. Releases build binaries for every platform in the table above.

## License and IP

- **License: GPL-3.0 (decided 2026-10-10).** This is the most common licence for open chess
  engines. The repo starts private and is published later (date open).
- GPL-3.0 lets us study and adapt code from GPL engines (Stockfish, Ethereal, Berserk and others).
  The learning goal means we write our own code anyway; when an idea or code comes from a specific
  engine, the comment names the source and its licence. No code from engines with incompatible or
  unclear licences.
- **Dependencies:** chess logic (board, move generation, search, evaluation, book reader) is
  written from scratch, with no chess crates. Tooling crates are fine (line editing, terminal
  colour, argument parsing, property testing, benchmarks) if their licence is GPL-3.0-compatible.
  `cargo deny` checks licences in CI. Syzygy probing is the one possible exception (see Open questions).
- **Data and nets:** tuning data and any NNUE nets are generated from our own engine's self-play,
  so we own them outright. No Stockfish or Leela nets or training data.
- Test books, EPD suites and reference engines are downloaded by scripts, with their source and
  licence noted, and are not committed unless their licence allows it.
- The engine needs a name that doesn't clash with existing engines (the CCRL and CPW engine lists)
  or trademarks.

## Milestones

| Milestone | Scope | Gate |
| --- | --- | --- |
| M0 · Foundations | Workspace, CI, licence checks, board, FEN, move generation, magics, perft (stages 1–3) | All perft suites exact; ≥100 Mnps perft on M1 |
| M1 · Playable | Zobrist, UCI, alpha-beta, iterative deepening, time management, quiescence; shell basics: board display, play a full game, thinking display, `perft`, `bb` (stages 4–7) | Plays complete games in Cute Chess and the shell with no illegal moves or time losses |
| M2 · Classical search | TT, move ordering, PVS, null move, LMR, pruning (stages 8–14) | ~2200 CCRL by gauntlet; every technique has an SPRT result in the ledger |
| M3 · Evaluation | Full HCE, eval trace, Texel tuner, self-play data generation, `eval`/`explain`/`tree`/`stats` (stages 15–16) | ≥2400 CCRL by gauntlet |
| M4 · Complete engine | Lazy SMP, Syzygy, Polyglot, Chess960, MultiPV, pondering, levels, XBoard, Lichess bot (stages 17–20) | GUI compatibility matrix passes; levels calibrated |
| M5 · v1.0 release | Strength work towards 2600–2700, book finished, release binaries, robustness run | ≥2400 CCRL Blitz (goal 2600+); 10,000-game robustness run clean; book complete for stages 1–20 |
| M6 · NNUE (stretch) | NNUE inference with SIMD, our own trainer, self-play data, HCE kept as a mode (stage 21) | Beats the HCE version by SPRT; book chapter done |

## Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Not enough test compute for SPRT | Changes are guessed, not measured; strength stalls | Short tests in cloud sessions; long tests on the Mac overnight; an OpenBench instance if needed |
| Subtle move generation or hashing bugs | Wrong moves, crashes, weird draws | Perft suites, property tests, fuzzing, hash checks in debug builds |
| Time losses in particular GUIs | Bad experience, rating list exclusion | `Move Overhead`, conformance tests, very short time controls in tests, GUI matrix |
| Book and code drift apart | Learners get confused | Code included into the book from source with anchors; book built in CI; chapter per tag |
| Readability vs speed | Either a slow engine or unreadable code | Clarity by default; optimise only measured hot spots, with comments explaining them |
| Strength limiting feels unnatural | Players don't enjoy low levels | MultiPV-based move choice; calibration; Carlos playtests the levels |
| Syzygy probing is a lot of work | Tablebases slip | Decide between a crate and our own port early (Open questions) |
| CI minutes on a private repo | CI throttled or costly | Linux on PRs; macOS/Windows nightly and on tags |

## Decisions

- [x] Language: Rust, stable toolchain (2026-10-10).
- [x] Strength target: 2400–2700 CCRL; v1.0 gate ≥2400 Blitz, goal 2600+ (2026-10-10).
- [x] Evaluation: tuned HCE for v1; NNUE as a stretch milestone, with HCE kept as a mode (2026-10-10).
- [x] Licence: GPL-3.0; repo private at first (2026-10-10).
- [x] Protocols: UCI, XBoard/CECP, Chess960, Lichess bot setup (2026-10-10).
- [x] Shell: rich line-based REPL; no full-screen TUI in v1 (2026-10-10).
- [x] Learning features: book, milestone tags, explain commands, technique switches (2026-10-10).
- [x] PRD lives in `docs/PRD.md` in the repo (2026-10-10).

## Open questions

- [ ] Engine name (check the CCRL and CPW engine lists and trademarks).
- [ ] Syzygy probing: use the pure-Rust `shakmaty-syzygy` crate (GPL-3.0, compatible) behind an
      adapter, wrap Fathom (C, MIT) via FFI, or write our own port as a book chapter?
- [ ] Long-test machine: Carlos's Mac overnight, a dedicated Linux box, or a cloud OpenBench instance?
- [ ] Which commercial GUIs Carlos has (ChessBase/Fritz, Hiarcs, Shredder) for the compatibility matrix.
- [ ] When to make the repo public, and whether to publish the book on GitHub Pages then.
- [ ] Is a full-screen TUI wanted after v1?
- [ ] Should the shell `match` command (mini tournament manager) be in v1 or later?
