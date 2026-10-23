# cb2026 — Product Requirements

A chess engine in Rust. Engine name: **cb2026** (ChessBrain 2026, decided 2026-10-10). The repository
stays `cjus/chessengine`; the binary, crate prefix and UCI `id name` are `cb2026`.
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

- No graphical interface and no full-screen terminal UI. cb2026 is an engine used under chess
  GUIs; the line-based shell is for playing, studying and debugging it (decided 2026-10-10,
  cjus/chessengine#9).
- No Monte Carlo tree search or large GPU neural networks (the AlphaZero/Leela family).
- No chess variants beyond Chess960.
- No code, nets or training data copied from other engines unless the licence is permissive and
  we credit it (see "License and IP").

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
              Zobrist hashing, FEN, SAN/UCI move notation, PGN, perft, and the
              game (`Game`: moves played, takebacks, when it is over, shared by
              the shell, the match tool and chessgui). No dependencies.
  engine/     search, evaluation (HCE; NNUE later), transposition table,
              time management, threads (Lazy SMP), tablebases, opening book,
              strength limiting. Exposes an Engine API with an info callback.
  protocol/   UCI and XBoard/CECP adapters over the Engine API.
  shell/      the interactive REPL: commands, board views, live thinking display.
  tools/      tuner (Texel), self-play data generation, bench, test helpers.
  nnue/       (M6) NNUE inference: the net's inputs, layers and integer
              weights, used by the engine. No I/O.
  trainer/    (M6) our own NNUE trainer: a CPU reference and a GPU backend
              (wgpu, Metal on the Mac). Not a dependency of the engine.
  cli/        the single `cb2026` binary: picks UCI, XBoard or shell mode.
book/         mdBook: "Building a chess engine in Rust".
docs/         PRD, Elo ledger, testing guide, GUI compatibility matrix.
tests/data/   perft suites, EPD test suites, bench positions.
scripts/      SPRT and gauntlet runners for the test Mac, release helpers.
```

**One binary, four modes.** GUIs launch the engine and send `uci` (or `xboard`) as their first
line, so the binary chooses its mode from that first line: `uci` → UCI, `xboard` → CECP, a JSON
message → the Chess Node Protocol (Solrac, below), anything else on an interactive terminal → the
shell. `--uci`, `--xboard`, `--cnp` and `--shell` force a mode, and `cb2026 bench` runs the
benchmark (see Testing).

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
| Move ordering | TT move, captures by MVV-LVA and SEE, killer moves, history heuristic, countermoves, continuation history, capture history (M5; off by default: three versions measured weaker), staged move generation |
| Pruning and reductions | Null-move pruning, ProbCut (M5; off by default, measured weaker), reverse futility pruning, futility pruning, late move pruning, history pruning (M5; off by default, measured weaker), late move reductions, larger reductions at expected cut nodes (M5), smaller reductions at expected all-nodes (M5; off by default, measured weaker), smaller reductions at nodes that are or have been on the principal variation (M5; off by default: no measured gain), larger reductions after many fail highs, in Stockfish 17's version (M5), larger reductions when the hash move is a capture (M5; off by default, measured weaker), razoring, SEE pruning in main and quiescence search, delta pruning, internal iterative reductions, mate distance pruning |
| Extensions | Check extension; singular extensions (should-have) |
| Static evaluation | Correction history: the static evaluation corrected by how far off it has been with the same pawns, learnt from the search (M5); tables by each side's pieces as well (M5; off by default: averaged with the pawn table they measured weaker, added to it no different) |
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

### Evaluation (NNUE, M6)

**Decided 2026-10-23 (Carlos):** cb2026 trains its nets with a trainer of its own, for its
teaching value, and the trainer uses the Mac's GPU through Metal.

- **The net's definition** (`crates/nnue`, no I/O, decided 2026-10-23; the crate's docs and
  chapter 21 have the details). Both the trainer and the engine use it:
  - **Inputs:** 768 per side, numbered 384 × (0 ours, 1 theirs) + 64 × piece type (pawn 0 to
    king 5) + square (a1 0 to h8 63), with the ranks flipped for Black's view. None depends on
    the king's square, so no move forces a full recompute.
  - **Layers:** a hidden layer of 256 per side, kept up to date move by move (the accumulator),
    then one output from the side to move's half and the other side's. The size is read from the
    file (any multiple of 32 up to 4096), so other sizes can be compared.
  - **Activation:** SCReLU (clamp to 0..1, then square); CReLU is also supported, so the two
    can be compared by SPRT on the same data.
  - **Integers:** first-layer weights and biases as `i16` × 255, output weights `i16` × 64,
    output bias `i32` × 255 × 64. Training clips every weight to ±1.98, so no legal position
    can overflow (a hidden value peaks at 33 × 505 = 16,665).
  - **File:** a 48-byte header that describes the net (format version, input set, hidden size,
    activation, scales, eval scale) with an FNV-1a checksum, then the numbers, little-endian,
    with one row of first-layer weights per input.
- **Eval scale:** the net's output is in cb2026's own centipawns, so the search's margins, tuned
  on the HCE, keep their meaning. The trainer fits the scale like the Texel tuner's `K`: `K` =
  1.19 on 40,304 self-play positions (2026-10-23), which is σ(cp / 146). Using the usual 400
  would let the score and result targets disagree and stretch the net's centipawns.
- **Inference speed:** integer arithmetic in plain safe Rust, written so the compiler
  vectorises it (NEON is always there on Apple Silicon). Hand-written SIMD, in a marked `unsafe`
  module, comes only if measuring shows the evaluation is a hot spot the compiler misses
  (CLAUDE.md rules 4 and 5); x86 releases would then need an AVX2 build or runtime CPU
  detection. The HCE stays as a switchable mode; `EvalFile` loads another net.
- **Data:** `datagen` self-play, with the HCE for the first net and each net's own games for the
  next, stores the search score as well as the game result, in a compact binary file. A first
  net needs about 100 million positions. `datagen` made about 190 a second per thread on the M1
  Max (2026-10-23), so the M3 at 9 threads makes about 8 million an hour: about 12 hours for the
  first net's data, a long run for the M3.
  - **Records** (`cb2026_nnue::record`, decided 2026-10-23): 32 bytes per position, so 100
    million take 3.2 GB. Each keeps the whole position (pieces, side to move, castling rights
    as "rooks that can still castle", so Chess960 works, the en passant square and the
    halfmove clock; not the fullmove number), the search score and the result from White's
    side, the best move, and a version byte. No file header, so files can be joined and
    shuffled. The layout follows marlinformat (marlinflow, no licence stated; the idea only).
    The inputs are computed from the position at training time, so a net with other inputs
    trains on the same records.
  - `datagen --format records` writes them (`--format text`, the default, still writes the
    Texel tuner's `FEN;result` lines), and the `records` tool checks a file and shows what it
    holds.
  - **Balanced starts** (2026-10-23): for records, a start (opening plus 8 random moves) that
    scores beyond 200 cp is drawn again, up to 20 times (`--max-start-score`, `off` for none).
    On 600 games the records within ±100 cp went from 18.5% to 30.0%, those beyond ±500 from
    37.5% to 25.3%, and White's wins from 54% to 41%, at the same speed.
- **Trainer** (`crates/trainer`, a binary): forward and backward passes written by hand, with no
  machine-learning framework, so the book can walk through every gradient. Two backends compute
  the same thing:
  - a **CPU reference**, the version the book explains, its gradients checked against finite
    differences;
  - a **GPU backend** on `wgpu` (MIT OR Apache-2.0), which runs on Metal on the Mac (and Vulkan
    or DirectX 12 elsewhere), with WGSL compute kernels, each tested against the CPU reference on
    the same batch. wgpu's API is safe Rust, so the trainer needs no `unsafe`.

  bullet (MIT), the trainer many engines use, would be allowed by "License and IP", but its
  training code wouldn't be ours to explain, so it isn't used.
- **Build and tests:** the trainer is not among the workspace's default members, so `cargo
  build` and the `cb2026` binary never compile wgpu. `scripts/check.sh` checks it with the rest;
  its GPU tests sit behind a cargo feature and run on the Mac, since the nightly Linux CI has no
  GPU.
- **Nets** are not committed: they are published as release assets and fetched by a script, as
  opening books are. A release build embeds the default net.
- **Order:** the net's definition (inputs, layers, the integer format) comes first, in
  `crates/nnue`, since the trainer and the engine must agree on it. Then `datagen`'s scores and
  file format, the CPU trainer and the GPU backend, alongside the engine's inference; none of
  these change the engine's play until a net is switched on. The first SPRT, net against HCE,
  needs both halves.

### Extras

| Feature | Priority | Notes |
| --- | --- | --- |
| Syzygy endgame tablebases | Must | WDL probes in search, DTZ at the root; `SyzygyPath`, `SyzygyProbeDepth`, `SyzygyProbeLimit`. Probed through `shakmaty-syzygy`, see below |
| Polyglot opening book | Must | Our own `.bin` reader; `OwnBook` and `BookFile` options; off by default so testing isn't affected |
| MultiPV | Must | Up to 256 lines |
| Strength limiting | Must | `UCI_LimitStrength`, `UCI_Elo`, `Skill Level` (see "Levels") |
| Win/draw/loss output | Should | `UCI_ShowWDL`, from a model fitted to our self-play results |
| Pondering | Must | `Ponder` option, `go ponder` / `ponderhit` |
| Bench | Must | Fixed list of positions searched to a fixed depth; prints total nodes and speed. The node count is the search's fingerprint |

### Tablebase probing

Decided 2026-10-10 (confirmed by Carlos in cjus/chessengine#8). Syzygy files use a complex compressed format, so
the probing code is a real choice:

| Option | For | Against |
| --- | --- | --- |
| `shakmaty-syzygy` (pure Rust, GPL-3.0) | Builds with cargo on every platform; no C toolchain, no `unsafe` FFI; mature (used by Lichess's tablebase server); its GPL matched cb2026's licence until 2026-10-20, and now makes a binary built with it GPL as a whole ("License and IP") | Brings in the `shakmaty` chess library, and each probe converts our position to its type |
| Fathom (C, MIT) through FFI | Used by many engines; takes bitboards directly; fast | C build on three platforms (MSVC is awkward), `unsafe` FFI, C code in a Rust teaching project |
| Our own port | Best learning value | 2,000+ lines of tricky decompression code; a big bug risk for no strength gain |

**Recommendation: `shakmaty-syzygy`**, as the one documented exception to the "no chess crates"
rule. It sits behind a small `Tablebase` trait in `crates/engine` and a default-on cargo feature
`syzygy`, so search code never sees `shakmaty` types and the prober can be replaced later.
Probes only happen in positions with at most `SyzygyProbeLimit` pieces, where converting the
position costs little next to reading the table. Tests check known results (won, drawn and
lost positions) using the small 3- and 4-piece tables in CI and the 5-piece tables locally;
the tables are downloaded by a script, never committed. The book chapter explains how tablebases
are built and probed; writing our own prober is a possible post-1.0 chapter.

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
| UCI_Elo | spin | 2450 | 650–2450: level 0's and level 19's ratings at 40+0.4, rounded inward (cjus/chessengine#106) |
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

### Chess Node Protocol (Solrac)

Solrac, the 3D board that teaches (`cjus/chessgui`), reaches cb2026 through the
[Chess Node Protocol](https://github.com/cjus/chess-node-protocol) (CNP): JSON-RPC 2.0 over
stdin/stdout, which Solrac's milestone M1b defines (cjus/chessgui#98). Every request carries the
whole position and every option, so cb2026 can one day be one node of a distributed engine.
`crates/cnp` (`cb2026-cnp`) answers it. A search has the limits UCI's `go` has, plus a level,
MultiPV, root moves, technique switches (the shell's, by the same names), a deterministic mode
and progress reports. `engine/techniques` and `engine/levels` list the switches and the measured
levels, and `engine/evaluate` gives a position's evaluation term by term, as the shell's `eval`
does, so Solrac's chat can say why cb2026 likes a position (cjus/chessgui#145). Every capability is offered except a root score window. Every conformance session in the
protocol's repository runs against the real binary in `cargo test` (`crates/cli/tests/cnp.rs`).

### Lichess bot (must)

A documented setup (`docs/lichess.md`) for running the engine as a Lichess BOT account with
[lichess-bot](https://github.com/lichess-bot-devs/lichess-bot): engine config, recommended options,
and which time controls to accept. lichess-bot is a separate project; we don't bundle it.

### Compatibility matrix

A checklist in `docs/gui-compat.md`, re-run at each release, covering: load, options dialog,
analysis with MultiPV, play a timed game, Chess960 game, pondering, and adding/removing the engine.
GUI testing happens on Carlos's Apple Silicon Mac (decided 2026-10-10), so the matrix is
Mac-first. **Open-source GUIs are the main targets**: they're free, they're what learners will
use, and their source shows exactly how they drive an engine. Commercial GUIs come second.
Status as of 2026-10-10 (details and sources in the Research log):

| GUI / tool | Platform | License | Role | Who checks |
| --- | --- | --- | --- | --- |
| cb2026 `match` tool; fastchess, cutechess-cli | Linux/macOS | Ours (GPL-3.0); open source (MIT / GPL-3.0) | Automated matches, SPRT, gauntlets | Claude (scripted, every release) |
| En Croissant | macOS (.dmg) | Open source (GPL-3.0) | **Primary GUI**: modern analysis and play, any UCI engine. Installed and running on the M1 Max (2026-10-10, cjus/chessengine#12) | Carlos |
| Cute Chess | macOS (Apple Silicon build exists) | Open source (GPL-3.0) | **Primary for engine-vs-engine** games and tournaments | Carlos |
| Scid vs. PC / Scid | macOS | Open source (GPL-2.0) | Database and analysis; UCI and XBoard engines | Carlos |
| XBoard | macOS (Homebrew) | Open source (GPL-3.0) | The reference CECP GUI | Carlos (CECP mode) |
| Nibbler | macOS (community package) | Open source (GPL-3.0) | Analysis GUI built for Lc0, works with UCI engines | Optional |
| ChessX | macOS (Intel build, runs under Rosetta 2) | Open source (GPL-2.0) | Database with engine analysis | Optional |
| Banksia | macOS | Free, closed source | Tournament GUI | Optional |
| HIARCS Chess Explorer (Pro) | macOS | Commercial | **Carlos's commercial Mac GUI** (cjus/chessengine#10) | Carlos. The 2026 Mac release runs natively on Apple Silicon; check that it loads cb2026 as an external UCI engine at the first GUI check |
| ChessBase for Mac | macOS | Commercial | Not planned | Carlos uses ChessBase on Windows instead (cjus/chessengine#10) |
| Shredder 13 | macOS | Commercial | Not planned | Accepts UCI engines, but the last major release was 2016 |
| ChessBase (Windows) | Windows | Commercial | **Carlos's commercial Windows GUI** (cjus/chessengine#10) | Carlos, on his Windows machine, once the Windows build runs well (cjus/chessengine#11); until then UCI conformance tests and the Cross-platform CI job stand in |
| Arena | Windows | Free | Optional | Carlos, on the same Windows machine, if time allows |
| lichess-bot | Linux/macOS | Open source (AGPL-3.0) | Online play on Lichess | Claude sets it up; Carlos runs it |

Claude can't operate GUIs, so it also maintains a **protocol conformance test**: a script that
plays scripted UCI and XBoard sessions against the binary and checks every response, including
awkward GUI habits (options sent mid-search, `position` without `ucinewgame`, very short clocks,
`stop` straight after `go`, and Windows line endings).

## The interactive shell

Started with `cb2026` in a terminal (or `cb2026 --shell`). It's a line-based REPL
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
| Tools | `bench`, `selfplay <n> [tc]`, `match <engine-path> <n> [tc]` (a small match against another UCI engine: must-have in v1, cjus/chessengine#9; see below), `uci` / `xboard` (switches the session to that protocol), `help [command]`, `quit` |

### The `match` command

Decided 2026-10-10 (cjus/chessengine#9): in v1. `match <engine-path> <n> [tc]` plays `n` games
between cb2026 and another UCI engine from inside the shell, alternating colours, and shows the
running score, a live board for the current game, and the final result with an Elo estimate and
error margin. Games are saved as PGN. It's for quick, friendly comparisons and for learners
("does my change beat the previous stage?"); rigorous testing uses the `match` tool's SPRT.
It needs the shell to drive a second engine over UCI, so it arrives in M4, with an optional
opening file (EPD or PGN) and Chess960 support.

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
  controlled randomness, plus **a person's horizon**: many of a weaker level's moves come from a
  search of one ply (below level 8) or two, with no quiescence search, so it blunders the way
  people do, by not seeing the reply or the move after it. The book explains why "search less"
  alone doesn't produce human-like play.
- **On a human scale, fitted, not guessed:** level 0 is a Lichess rapid rating of 600 and each
  level 100 more, to 2500 at level 19, and each level's chance of a person's horizon is fitted
  so that it blunders as often as people of that rating, counted as Lichess counts them in
  Lichess's own games (the `mistakes` tool; chapter 19). `UCI_Elo` (600–2500) is on that scale,
  and CNP's `engine/levels` names it. It is an estimate from blunders; games against people
  would confirm it. Until 2026-10-21 the levels were rated against engines on the CCRL scale
  instead (`scripts/mac-levels.sh`, kept in the book as the record), which suited engines but
  not the people the levels are for (cjus/chessgui#113).
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
   its tests, matches its book chapter and, from stage 4 on, plays legal chess through UCI, so a learner can run
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
| Unit and property tests | make/unmake restores the position; incremental hash equals a full recompute; FEN round-trips; SAN parsing | `scripts/check.sh` before every PR merges; nightly CI; Claude in session |
| Perft suites | Move generation is exactly right (standard and Chess960) | Every depth: `scripts/check.sh` and nightly CI |
| Fuzzing (`cargo fuzz`) | The FEN, SAN, UCI and XBoard parsers never panic on bad input | Locally and weekly |
| Protocol conformance | Correct UCI/XBoard behaviour, including GUI quirks | `scripts/check.sh`; nightly CI |
| Bench | Search didn't change by accident: same node count = same search | Locally; recorded in every commit message |
| Tactical suites (EPD) | Sanity check: e.g. most WAC positions solved in 1 s | Before tags |
| SPRT self-play | Did this change gain Elo? | Claude's session (short), Carlos's Mac (long) |
| Gauntlet vs reference engines | Where we are on the CCRL scale | Each milestone gate |

**SPRT rules.** Every change that affects search or evaluation is tested with cb2026's own
`match` tool (`cb2026-tools`; fastchess works too) using a
sequential probability ratio test, against the current `main`:
- gainers: `elo0=0, elo1=5` at short time control (8+0.08 s), and at long time control (40+0.4 s) for
  pruning and time-management changes;
- simplifications and refactors: non-regression bounds `[-5, 0]`;
- balanced opening book, games played in pairs with colours swapped.

Every commit message carries `Bench: <nodes>`. A PR changing search or evaluation includes its
SPRT result (Elo ± error, games played, LLR) or says "No functional change" with an unchanged bench.

**Rating.** At each gate, a gauntlet of several thousand games against open-source engines
whose CCRL Blitz ratings bracket our target. The `gauntlet` tool holds their ratings fixed and
fits cb2026's by maximum likelihood. The engines, chosen on 2026-10-17 (cjus/chessengine#36),
with their CCRL Blitz ratings that day:

| Engine | CCRL Blitz | Language |
| --- | ---: | --- |
| Blunder 5.0.0 | 2018 ± 20 | Go |
| Stash 15.3 | 2174 ± 19 | C |
| Zahak 3.0 | 2364 ± 19 | Go |
| Velvet 1.0.0 | 2513 ± 17 | Rust |
| Blunder 8.5.5 | 2663 ± 11 | Go |
| Weiss 1.0 | 2896 ± 17 | C |
| Stash 27.0 | 3049 ± 16 | C |

They are about 150 Elo apart, by five authors, and cover every gate from M2 (about 2200) to
the v1.0 goal (2600+). Of the first candidates, Rustic (its best version is 1794), Fruit 2.1 (not
on the Blitz list) and Ethereal (open-source versions from 3333) don't fit the range.
`scripts/fetch-gauntlet-engines.sh` builds them from source at pinned commits, for testing
only; they are never included in the repo. CCRL rated x86 builds, so each version was checked
to behave the same built for Apple Silicon (the script says how); Weiss 0.10 and Berserk 2.0.0,
the first choices for two of the places, failed that check. The gauntlets run at 8+0.08, much
faster than CCRL Blitz's 2'+1", so their ratings are approximate; a run at 40+0.4 is closer and
takes five times as long.

**For the strength levels**, which go down to `UCI_Elo` 650, five weaker engines join the
seven (chosen on 2026-10-18, cjus/chessengine#101; same CCRL snapshot):

| Engine | CCRL Blitz | Language |
| --- | ---: | --- |
| Fatalii 0.1.1 | 1058 ± 22 | Rust |
| Walleye 1.3.0 | 1266 ± 21 | Rust |
| Arche 0.3.8 | 1451 ± 20 | Rust |
| Blunder 4.0.0 | 1616 ± 22 | Go |
| Rustic Alpha 3.0.0 | 1794 ± 16 | Rust |

With the gauntlet's lower four, that is one engine every 165–225 points from 1058 to 2513. The
two engines below 1000 that were tried failed the Apple Silicon check (Megalodon 1.0.0 reads
past an array, Algae 1.0 builds only for x86), so the lowest levels are rated from above. Each Skill Level plays the
four engines nearest its expected rating (`scripts/mac-levels.sh`, cjus/chessengine#103 and
#106). Two of the five have bugs of their own that lose games, which count as losses: Walleye
1.3.0 ignores en passant captures in the moves it is sent (about 1% of its games), and Arche
0.3.8 sometimes answers `bestmove 0000` with legal moves on the board (2 of its 1,400 games at
40+0.4, none at 8+0.08).

### Test machine

Decided 2026-10-10: long tests run on Carlos's Apple Silicon Mac, short ones in Claude's cloud
sessions. Changed 2026-10-12: long tests run on an Apple M3 Max on Carlos's network
(cjus/chessengine#62). The M1 Max that ran them until then is Carlos's working machine, where
Claude Code runs, and is no longer used for tests.

**The test machine:** Apple M3 Max, 14 cores (10 performance, 4 efficiency), 36 GB RAM
(cjus/chessengine#62). Until 2026-10-12 it was an Apple M1 Max, 10 cores (8 performance, 2
efficiency), 64 GB RAM. This gives:
- **SPRTs and gauntlets:** one game per performance core minus one (9 on the M3 Max, 7 on the
  M1 Max),
  single-threaded engines, 16 MB hash each (two engine processes per game, well within memory).
- **Lazy SMP tests:** up to one thread per performance core on one engine against one opponent,
  run on their own, not mixed with other tests.
- **Tablebases:** the full 3-4-5-piece Syzygy set (about 1 GB) fits easily; the 6-piece set
  (about 150 GB on disk) works only with an external drive and is not needed for v1.
- **Speed baseline:** speed figures name the machine they were measured on. The ≥100 Mnps perft
  gate (278.8 Mnps) and the `bench` speeds up to 2026-10-12 are from the M1 Max; later ones are
  from the M3 Max. Its setup check (2026-10-12): the perft suites exact at 369.6 Mnps
  (665,777,044 nodes in 1.8 s), and `bench` 765380 nodes at 2.13 Mnps (the M1 Max: about
  1.7).

- **`scripts/mac-sprt.sh <PR>`** (Claude writes it, Carlos runs it): builds the PR branch and
  `main` in release mode, runs the SPRT with the `match` tool, and posts the result to the PR as a
  `<!-- sprt -->` comment through `gh`. Carlos only starts it and lets it run overnight;
  `scripts/mac-gauntlet.sh` does the same for rating gauntlets.
- **Performance cores only:** concurrency = performance cores − 1, so 9 on the M3 Max (`sysctl
  hw.perflevel0.physicalcpu` reports 10) and 7 on the M1 Max. Efficiency cores run at a different speed and would skew results
  at a fixed time control.
- **Steady conditions:** `caffeinate -i` so the Mac doesn't sleep, on mains power, other heavy
  apps closed. A laptop may throttle under hours of full load; a Mac mini or Studio is steadier.
- **Throughput:** at 8+0.08 a game takes about 26 s whatever the CPU, because the clock sets
  its length. 7 games at once on the M1 Max gave about 970 games an hour (measured); 9 on the
  M3 Max should give about 1,250. Early stages gain tens of Elo and their SPRTs finish in minutes, so they run
  in cloud sessions (4 cores, 3 games at once). Late-stage +3–5 Elo changes need 10,000–40,000
  games: one or more nights on the Mac.
- **If the queue backs up** (likely during M5 strength work): set up OpenBench, the open-source
  distributed testing framework, with the Mac as one worker plus a rented multi-core Linux server.
  An AWS Batch setup in the PNX Studios account (Graviton spot instances, about $0.08 per 1,000
  games) was proposed in cjus/chessengine#61 and is on hold.
- x86-64 builds are tested for correctness in CI and for speed in cloud sessions; strength is
  measured on the test Mac.

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
| Long SPRT, LTC, gauntlets | Writes `scripts/mac-sprt.sh` and `mac-gauntlet.sh`, reads the posted results | Starts them on the Mac; results post to the PR |
| GUI compatibility checks | Protocol conformance tests | Checks real GUIs, especially commercial ones |
| Playing and judging the shell UX | No | Yes |

**Workflow:** `claude/<topic>` branches from the latest `main`, one PR each;
`scripts/check.sh` (fmt, clippy, tests, every perft depth, conformance, docs, licences and
advisories, book build) must pass before it merges; the PR states bench and SPRT results. Each
stage ends with a tag, a book chapter and an Elo ledger entry. `main` is protected by the
ruleset `main: PR`: every change arrives through a PR, and force pushes and branch deletion are
blocked. From 2026-10-10 to 2026-10-22 it was `main: PR + CI` and also required a passing `CI`
check on GitHub (one job since 2026-10-21; three checks before).

**CI cost.** GitHub Actions minutes are limited on private repos, and macOS and Windows minutes
have historically cost 10× and 2× Linux minutes. So CI runs on Linux; macOS and Windows builds run
weekly and on release tags. Releases build binaries for every platform in the table above. The
account's minutes, shared with chessgui, ran out on 2026-10-21 (cjus/chessgui#73), about 1,800 of
them having gone here in three weeks. Since then:
- CI is one job (GitHub bills each job to the next whole minute).
- GitHub checks neither PRs nor merges (since 2026-10-22, Carlos: the code is checked locally,
  with `scripts/check.sh`). `main` is checked nightly, only if it changed since the last good
  run.
- Only `main` saves build caches.
- Fuzzing and the macOS and Windows builds run weekly, only if `main` changed.

The repository becoming public (planned, cjus/chessgui#71) makes its CI free.

## License and IP

- **License: `MIT OR Apache-2.0` (decided 2026-10-20, cjus/chessgui#71; replaces GPL-3.0,
  decided 2026-10-10).** Carlos's rule: the open-source licence that gives us the most freedom
  while staying compliant. Solrac, the 3D board, is to be closed source and still use cb2026,
  through the Chess Node Protocol for the engine and in-process for the rules (`cb2026-chess`);
  cb2026 is also to become a node of a distributed engine. The dual licence is the Rust
  convention: each user picks MIT, which is compatible with every licence (GPL-2.0-only
  included), or Apache-2.0, which adds a patent grant from contributors and grants no trademark
  rights. Contributions come in under the same terms (README, "Contribution"). The book is
  under the same licence, as the Rust book is. © Carlos Justiniano.
- **The one GPL part is Syzygy probing.** `shakmaty-syzygy` and `shakmaty` are
  GPL-3.0-or-later, so a `cb2026` binary built with the `syzygy` feature (on by default, and in
  the releases) is GPL-3.0-or-later as a whole and ships with `crates/syzygy/LICENSE-GPL-3.0`;
  `--no-default-features` builds one with no GPL code. A permissive prober (Fathom, MIT) would
  remove the exception; see "Tablebase probing".
- **Code from other engines: ideas yes, GPL code no.** Ideas aren't covered by copyright, so the
  book and the code keep learning from Stockfish, Ethereal, Berserk and the rest, and say so.
  Code may be adapted only from permissive sources (MIT, Apache-2.0, BSD, ISC, Zlib, public
  domain), with the comment naming the source and its licence. Before the change, cb2026's
  code was checked (2026-10-20, again 2026-10-21): it is all Carlos's and Claude's, except the
  Polyglot keys, which were copied from python-chess (GPL) and are now taken from the format's
  published description (`book_keys.rs`), and the Syzygy crate above.
- **Public:** the repo starts private and goes public around October 2027 (decided
  2026-10-10), or before Solrac is given to anyone if that comes first (cjus/chessgui#71); the
  book is published on GitHub Pages at the same time.
- **Dependencies:** chess logic (board, move generation, search, evaluation, book reader) is
  written from scratch, with no chess crates. Tooling crates are fine (line editing, terminal
  colour, argument parsing, property testing, benchmarks) if their licence is permissive, or
  MPL-2.0, which covers only its own files. `cargo deny` checks licences in CI. The one
  exception is `shakmaty-syzygy` for tablebase probing, kept behind an adapter (see
  "Tablebase probing").
- **Data and nets:** tuning data and any NNUE nets are generated from our own engine's self-play,
  so we own them outright. No Stockfish or Leela nets or training data. Nets are published as
  release assets, not committed (see "Evaluation (NNUE, M6)").
- Test books, EPD suites and reference engines are downloaded by scripts, with their source and
  licence noted, and are not committed unless their licence allows it.
- Name: a web search on 2026-10-10 found no chess engine called cb2026. Check the CCRL and CPW
  engine lists and trademarks again before the repo goes public.

## Milestones

| Milestone | Scope | Gate |
| --- | --- | --- |
| M0 · Foundations | Workspace, CI, licence checks, board, FEN, move generation, magics, perft (stages 1–3) | All perft suites exact; ≥100 Mnps perft on the M1 Max |
| M1 · Playable | Zobrist, UCI, alpha-beta, iterative deepening, time management, quiescence; shell basics: board display, play a full game, thinking display, `perft`, `bb` (stages 4–7) | Plays complete games in En Croissant and the shell with no illegal moves or time losses |
| M2 · Classical search | TT, move ordering, PVS, null move, LMR, pruning (stages 8–14) | ~2200 CCRL by gauntlet; every technique has an SPRT result in the ledger |
| M3 · Evaluation | Full HCE, eval trace, Texel tuner, self-play data generation, `eval`/`explain`/`tree`/`stats` (stages 15–16) | ≥2400 CCRL by gauntlet |
| M4 · Complete engine | Lazy SMP, Syzygy, Polyglot, Chess960, MultiPV, pondering, levels, XBoard, Lichess bot, shell `match` command (stages 17–20) | GUI compatibility matrix passes; levels calibrated |
| M5 · v1.0 release | Strength work towards 2600–2700, book finished, release binaries, robustness run | ≥2400 CCRL Blitz (goal 2600+); 10,000-game robustness run clean; book complete for stages 1–20 |
| M6 · NNUE (stretch) | NNUE inference in vectorised integers, our own trainer (CPU reference and GPU via Metal), self-play data, HCE kept as a mode (stage 21; "Evaluation (NNUE, M6)") | Beats the HCE version by SPRT; book chapter done |

**Gate results.** M0 passed on 2026-10-11: every perft suite exact (665,777,044 nodes, 0
failures) at 278.8 million nodes/s on the M1 Max (cjus/chessengine#20).

M2 passed on 2026-10-18: `stage-14-pruning` rated **2442 ± 11** against the seven reference
engines, 800 games each (5,600 in all, +2058 −2746 =796) at 8+0.08 on the M3 Max
(cjus/chessengine#36), against a gate of about 2200. Its performance against each engine
ranged from 2395 (against Blunder 5.0.0) to 2494 (Blunder 8.5.5). The ± is the statistical
error only: it holds the reference ratings fixed, and the time control is much faster than
CCRL's (see "Rating"). cb2026 had no time losses, illegal moves or failures; Stash 15.3 had
one time loss and five failures. Stages 8–14 each have an SPRT result in
`docs/elo-ledger.md`.

M3 passed on 2026-10-18: `stage-16-tuning` rated **2669 ± 10** in the same gauntlet (5,600
games, +2998 −1708 =894; cjus/chessengine#36), against a gate of 2400 or more. Stage 16's
default evaluation is stage 15's hand-set one (the tuned values weren't adopted), so this rates
the full hand-crafted evaluation. Its performances ranged from 2588 (against Blunder 5.0.0,
which it beat 96.4% of the time, where a rating says least) to 2695 (Stash 27.0). That is 227
above stage 14, close to the +192 ± 45 that stage 15's SPRT measured in self-play. cb2026 again
had no time losses, illegal moves or failures; Stash 15.3 had one time loss and one failure.

`main` (at `40751d9`) rated **2714 ± 10** in the same gauntlet on 2026-10-18 (+3181 −1513 =906;
cjus/chessengine#36), above the v1.0 rating gate (2400 or more, goal 2600+); the release
measures it again with the robustness run. That is only 45 ± 14 above stage 16. The M5 changes
that passed their SPRTs add up to about +136 Elo in self-play, so against other engines they
gained about a third of that: self-play SPRTs find gains reliably but overstate their size.

`main` (at `a422e69`) rated **2782 ± 11** in the same gauntlet on 2026-10-21 (+3432 −1215 =953;
cjus/chessengine#131), 68 ± 15 above `main` three days before. Two changes passed their SPRTs
in between, correction history (+28.0) and double extensions (+13.0), about +41 in self-play:
this time the gauntlet found more than self-play, not a third of it. Over M5 so far, from stage
16's 2669, the gauntlets found +113 where the SPRTs add up to about +177. Its performances
ranged from 2689 (against Blunder 5.0.0, beaten 97.9% of the time) to 2818 (Zahak 3.0). cb2026
had no time losses, illegal moves or failures; Stash 15.3 had one failure.

The v1.0 robustness run was **clean** (2026-10-22, cjus/chessengine#146). `main` at `9cff7fc`
played itself 10,000 times on the M3 Max, 9 games at a time with the default `Move Overhead`:
6,000 games at 1+0.01, 3,000 at 10+0.1 and 1,000 at 40/60. None had a time loss, an illegal
move, a failure or an aborted game. `main` has changed since only in the weaker levels
(cjus/chessengine#151); full strength searches the same tree (the bench is unchanged). With the
gauntlet passed, the GUI compatibility matrix (cjus/chessengine#37) is v1.0's last gate.

M4's levels are calibrated (2026-10-19, cjus/chessengine#106): Skill Levels 0–19 rate **629 to
2483** at 40+0.4 (± 23 to 55), and `UCI_Elo` maps onto those ratings
(`crates/engine/src/levels.rs`, chapter 19). Every other level played 800 games against the
four reference engines nearest it, 8,800 games in all, and the odd levels are interpolated.
The first calibration, at 8+0.08 on 2026-10-18 (cjus/chessengine#103), rated the levels 752 to
2599. Levels limit nodes, so against opponents given more time they rate lower: 74 to 272
lower at 40+0.4, which is closer to CCRL's 2'+1". `UCI_Elo` now runs from level 0's rating to
level 19's, rounded inward to 50: 650–2450 (Carlos, cjus/chessengine#106). M4 still needs the
GUI compatibility matrix (cjus/chessengine#37).

## Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Not enough test compute for SPRT | Changes are guessed, not measured; strength stalls | Short tests in cloud sessions; long tests on the test Mac (the M3) overnight; OpenBench or AWS Batch (#61, on hold) if the queue backs up |
| Subtle move generation or hashing bugs | Wrong moves, crashes, weird draws | Perft suites, property tests, fuzzing, hash checks in debug builds |
| Time losses in particular GUIs | Bad experience, rating list exclusion | `Move Overhead`, conformance tests, very short time controls in tests, GUI matrix |
| Book and code drift apart | Learners get confused | Code included into the book from source with anchors; book built in CI; chapter per tag |
| Readability vs speed | Either a slow engine or unreadable code | Clarity by default; optimise only measured hot spots, with comments explaining them |
| Strength limiting feels unnatural | Players don't enjoy low levels | MultiPV-based move choice; calibration; Carlos playtests the levels |
| Tablebase crate changes or is abandoned | Tablebase support breaks | Behind a `Tablebase` trait and a cargo feature; Fathom or our own port can replace it |
| CI minutes on a private repo | CI throttled or costly | PRs checked locally; Linux nightly and macOS/Windows weekly, only if `main` changed; macOS/Windows also on tags |
| GPU training code is hard to debug | Wrong nets, or a trainer that stalls stage 21 | Every GPU kernel tested against the CPU reference on the same batch; the CPU trainer can train the first net on its own |
| Windows GUIs tested late | Windows-only GUI problems (ChessBase/Fritz, Arena) found late | Strict UCI conformance tests and Windows CI builds throughout; Carlos tests on a Windows machine once the Windows build runs well, before going public |

## Research log

Findings behind the decisions above, with their sources, so they can be rechecked later. Add a
dated entry whenever a decision rests on outside facts.

**2026-10-10: Commercial chess GUIs on macOS**

| Product | Finding | Confidence | To confirm |
| --- | --- | --- | --- |
| HIARCS Chess Explorer (Pro) | 2026 Mac release; macOS 10.14 or later including macOS 26; native on Intel and Apple Silicon (vendor page). A third-party listing says it supports external UCI engines | Vendor claim for the platform; UCI support from a third party | External UCI engine support in the current version (ask HIARCS or try the demo) |
| ChessBase for Mac | Native Mac version announced, built from the ground up, release November 2026. The announcement lists Fritz, Stockfish from the Engine Cloud and remote engines | From search summaries; fetching the article failed | Whether it can load a local UCI engine, once it ships |
| Shredder 13 | Last major release October 2016, Mac and Linux builds November 2016. Users report its GUI accepts any Mac UCI engine | Old and anecdotal; no native Apple Silicon build confirmed | Whether it still runs on current macOS (likely through Rosetta 2) |
| ChessBase/Fritz on Windows | Not native on Mac before the new version; workarounds were Boot Camp, a VM or Wine/CrossOver | Forum reports | Carlos has a Windows machine for this once the Windows build runs well |

Sources:
[HIARCS Mac chess software](https://www.hiarcs.com/mac-chess-software.html),
[HIARCS Chess Explorer Pro](https://www.hiarcs.com/chess-explorer-pro.html),
[ChessBase: "ChessBase finally on Mac"](https://en.chessbase.com/post/chessbase-finally-on-mac),
[Chess.com forum: Chess engines for Mac](https://chess.com/forum/view/chess-equipment/chess-engines-for-mac),
[Chessprogramming wiki: Shredder](https://www.chessprogramming.org/Shredder).

**2026-10-10: Open-source chess GUIs on macOS**

| Product | Finding | To confirm |
| --- | --- | --- |
| En Croissant | Free, GPL-3.0, built with Tauri; macOS .dmg from encroissant.org, macOS 10.13 or later; supports any UCI engine, added from a local file | Installed and running on Carlos's M1 Max (2026-10-10, cjus/chessengine#12) |
| Cute Chess | GPL-3.0; a third-party site provides a build for Apple Silicon and Intel; widely recommended for engine matches | Official vs third-party Mac build |
| Scid | A forum user reports it runs well on Apple Silicon | Which variant (Scid vs. PC or Scid) and its current Mac build |
| Nibbler | No official Mac build; the community project Nibbler-for-macOS packages it with Electron and Lc0 | Using it with a non-Lc0 UCI engine |
| ChessX | Open source; a compatibility tracker lists v1.6.10 as Intel-only | Whether it runs well under Rosetta 2 |
| Banksia | Free but closed source | — |

Sources:
[En Croissant: getting started](https://encroissant.org/docs/getting-started),
[En Croissant FAQ](https://www.mintlify.com/franciscoBSalgueiro/en-croissant/resources/faq),
[En Croissant engines](https://www.mintlify.com/franciscoBSalgueiro/en-croissant/features/engines),
[LinuxLinks: En Croissant review](https://www.linuxlinks.com/en-croissant-gui-chess-toolkit/),
[Chessengeria: Cute Chess for Mac](https://chessengeria.eu/post/cute-chess-for-mac),
[TalkChess: Chess GUI options for Apple Silicon](https://talkchess.com/viewtopic.php?p=968258),
[Nibbler for macOS](https://github.com/twoplan/Nibbler-for-macOS),
[Rosetta check: ChessX](https://rosettacheck.com/apps/a5naysyf),
[Chessprogramming wiki: Mac OS](https://www.chessprogramming.org/Mac_OS).

**2026-10-10: Tablebase probing crate**

`shakmaty-syzygy` on crates.io: version 0.28.1, last updated 2026-06-19, licence
GPL-3.0-or-later (compatible with our GPL-3.0). Source:
[crates.io/crates/shakmaty-syzygy](https://crates.io/crates/shakmaty-syzygy).
Recheck the version and maintenance status when stage 18 starts.

**2026-10-10: Engine name**

A web search for "cb2026" chess engine found no existing engine with that name. Recheck the CCRL
and Chessprogramming wiki engine lists and trademarks before the repo goes public.

**2026-10-10: Test throughput (estimate, not measured)**

At 8+0.08 a game takes about 20 s, so the M1 Max running 7 games at once (8 performance cores − 1)
plays about 1,200 games an hour; the 4-core cloud session (3 games at once) about 540. Replace
these with measured figures from the first real SPRT runs. Measured (2026-10-12): a game takes
about 26 s, and the M1 Max played about 970 games an hour.

## Decisions

- [x] Language: Rust, stable toolchain (2026-10-10).
- [x] Strength target: 2400–2700 CCRL; v1.0 gate ≥2400 Blitz, goal 2600+ (2026-10-10).
- [x] Evaluation: tuned HCE for v1; NNUE as a stretch milestone, with HCE kept as a mode (2026-10-10).
- [x] ~~Licence: GPL-3.0; repo private at first (2026-10-10). Crates declare `GPL-3.0-or-later`
      (cjus/chessengine#7, 2026-10-10).~~ Replaced on 2026-10-20 by the next entry.
- [x] Licence: `MIT OR Apache-2.0`, © Carlos Justiniano, so a closed-source Solrac can use
      cb2026; GPL only in a binary built with the `syzygy` feature; code adapted only from
      permissive sources; the repo private at first (cjus/chessgui#71, 2026-10-20; done in
      cjus/chessgui#98's first step, 2026-10-21).
- [x] NNUE training (M6): our own trainer for its teaching value, not bullet: gradients written
      by hand, a CPU reference and a GPU backend through `wgpu` (Metal on the Mac). Inference in
      `crates/nnue`, the trainer in `crates/trainer`, outside the default build; nets published
      as release assets, not committed (Carlos, 2026-10-23).
- [x] The first net (M6): 768 inputs per side, a hidden layer of 256, SCReLU (CReLU also
      supported), integer scales 255 and 64 with weights clipped to ±1.98, an eval scale fitted
      to cb2026's centipawns (about 146, not 400), a self-describing file with a checksum.
      Inference in safe, compiler-vectorised Rust; hand-written SIMD only if measured (Carlos,
      2026-10-23).
- [x] NNUE training records (M6): 32 bytes each, the whole position (castling as rooks that can
      still castle, for Chess960), score and result from White's side, best move and a version
      byte; no file header. Written by `datagen --format records` (Carlos, 2026-10-23).
- [x] Protocols: UCI, XBoard/CECP, Chess960, Lichess bot setup (2026-10-10).
- [x] Shell: rich line-based REPL. No full-screen TUI at all: cb2026 is an engine used under
      chess GUIs (cjus/chessengine#9, 2026-10-10).
- [x] Learning features: book, milestone tags, explain commands, technique switches (2026-10-10).
- [x] PRD lives in `docs/PRD.md` in the repo (2026-10-10).
- [x] Name: **cb2026** (ChessBrain 2026); the repo stays `cjus/chessengine` (2026-10-10).
- [x] Tablebases: `shakmaty-syzygy` behind a `Tablebase` trait (confirmed, cjus/chessengine#8, 2026-10-10).
- [x] Test machine: Carlos's Apple M1 Max (8 performance + 2 efficiency cores, 64 GB) for long
      SPRTs and gauntlets at 7 games at once, via scripts that post results to the PR; cloud
      sessions for short tests; OpenBench later if needed (2026-10-10).
- [x] Test machine moves to an Apple M3 Max (10 performance + 4 efficiency cores, 36 GB) on
      Carlos's network; the M1 Max is no longer used for tests. Running them on AWS Batch is on
      hold (cjus/chessengine#61, #62, 2026-10-12).
- [x] GUI testing is Mac-first and open-source-first: En Croissant and Cute Chess as the primary
      GUIs, plus Scid and XBoard (2026-10-10). Commercial GUIs: HIARCS Chess Explorer on the
      Mac and ChessBase on Windows; ChessBase for Mac and Shredder are not planned
      (cjus/chessengine#10, 2026-10-10).
- [x] En Croissant is installed on the test Mac and is the first GUI cb2026 is checked in, from
      M1 on (2026-10-10).
- [x] Repo goes public around October 2027, with the book on GitHub Pages (2026-10-10).
- [x] Shell `match` command (cb2026 against another UCI engine) is in v1, arriving in M4
      (cjus/chessengine#9, 2026-10-10).
- [x] Windows: Carlos tests on a Windows machine once the Windows build runs well
      (cjus/chessengine#11, 2026-10-10).
- [x] Match tool: cb2026 has its own UCI match runner with SPRT (`cb2026-tools`, `match`). Cloud
      sessions can't fetch fastchess (GitHub access is limited to this repo), a built-in tool
      needs nothing installed on the Mac, and it is part of the teaching material. fastchess
      still works for anyone who prefers it (2026-10-10).
- [x] `main` protected by the ruleset `main: PR + CI`: PR required, three CI checks required,
      no force pushes or deletion (cjus/chessengine#5, 2026-10-10). Since 2026-10-22 `main: PR`:
      no check required, PRs are checked locally to save Actions minutes (Carlos).
- [x] Gauntlet reference engines: seven open-source versions from 2018 to 3049 CCRL Blitz,
      built from source by `scripts/fetch-gauntlet-engines.sh` (see "Testing and strength
      measurement"; cjus/chessengine#36, 2026-10-17).
- [x] Level calibration engines: five more, from 1058 to 1794 CCRL Blitz (Fatalii 0.1.1, Walleye
      1.3.0, Arche 0.3.8, Blunder 4.0.0, Rustic Alpha 3.0.0), built by the same script
      (cjus/chessengine#101, 2026-10-18).
- [x] Levels calibrated at 40+0.4, not 8+0.08, since node-limited levels rate 74–272 lower
      there: every other level measured, the odd ones interpolated (cjus/chessengine#106,
      2026-10-18).
- [x] `UCI_Elo` range: 650–2450, level 0's and level 19's ratings at 40+0.4 rounded inward to
      50. It was 1000–2400 at first, then 800–2600 after the calibration at 8+0.08
      (cjus/chessengine#106, 2026-10-18).
- [x] Levels on a human scale (Carlos, cjus/chessgui#113, 2026-10-21): played at "Elo 1050" in
      Solrac, a level was "much stronger than a 1050 level player". Rated against engines on
      CCRL, it blundered 0.7 times in 40 moves where people rated about 1000 on Lichess blunder
      3.4 times. The levels now take a person's horizon on many moves and are fitted to blunder
      as people do at Lichess rapid ratings 600 to 2500, 100 a level; `UCI_Elo` 600–2500 is on
      that scale. Replaces the CCRL calibration and the 650–2450 range above for what the
      levels mean.
- [x] cb2026 speaks the Chess Node Protocol (`--cnp`, or a JSON first line) for Solrac, beside
      UCI and XBoard: one search at a time, every capability but the root window, the shell's
      switches as its techniques. The protocol's crates come from the private
      `cjus/chess-node-protocol` by git, with a read-only deploy key in CI
      (cjus/chessgui#98, cjus/chessengine#143, 2026-10-21).

## Open questions

None right now. New questions for Carlos go in GitHub issues labelled `needs-carlos` and are
recorded here when answered.
