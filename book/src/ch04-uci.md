# Zobrist hashing and talking to GUIs (UCI)

*Stage 4 · tag `stage-04-uci` · code in `crates/chess`, `crates/engine`, `crates/protocol`*

By the end of this stage cb2026 is a real, if hopeless, chess engine. A GUI can load it, send
it positions and get a legal move back: for now a random one. Three pieces make that possible.
**Hash keys** let us recognise positions, **algebraic notation** lets people read moves, and
the **UCI protocol** lets GUIs talk to the engine.

## Zobrist hashing

The search will meet the same position again and again, reached by different move orders,
and the repetition rule needs to spot recurring positions. Comparing whole positions is slow,
so each position gets a 64-bit fingerprint.

**Zobrist hashing** gives every feature a fixed random 64-bit number:
- one per (piece, square);
- one for "Black to move";
- one per castling right;
- one per en passant file.

The key is the XOR of the numbers for the features present:

```rust,ignore
{{#include ../../crates/chess/src/zobrist.rs:keys}}
```

Because `x ^ k ^ k == x`, the key updates incrementally. Moving a knight from g1 to f3 is
`key ^= N_g1 ^ N_f3`, a couple of instructions. cb2026 updates the key inside the same
functions that move pieces, so it can never fall out of step. A test walks the move tree from
every perft position and checks, at every node, that the incremental key equals one computed
from scratch.

Different positions *can* share a key, but with 64 random bits it's rare enough that engines
accept the risk. The transposition table in stage 8 also double-checks the moves it gets back.

The shell's `zobrist` command lists every part of the current key.

## Algebraic notation

UCI speaks in `g1f3`, but people write `Nf3`. **Standard algebraic notation (SAN)** names the
piece and destination:
- captures add `x` (`Bxe5`);
- promotions add `=Q`;
- castling is `O-O` or `O-O-O`;
- check and mate add `+` or `#`.

When two pieces of the same kind could reach the square, SAN adds the starting file (`Nbd7`),
else the rank (`R1e2`), else both.

```rust,ignore
{{#include ../../crates/chess/src/san.rs:to_san}}
```

Reading SAN is deliberately forgiving: annotations are ignored, `0-0` works, and so do
`e8Q` and over-specified moves like `Ng1f3`. The test is strict, though: every legal move in
every perft position must survive writing and reading back.

## The Engine API

The engine crate does no I/O, so the protocols and the shell drive it through one call:

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:search}}
```

- **`Limits`** says when to stop: a depth, a node count, a fixed time, the clock, or
  `infinite`.
- The **stop flag** is an `AtomicBool` that another thread can set at any moment.
- The **info callback** receives progress: depth, score, nodes and the best line. UCI prints
  it as `info` lines; the shell will show it as a live display.
- **`history`** carries the hash keys of earlier positions for repetition detection.

For now the "search" picks a random legal move. That's enough to test the whole path from a
GUI and back; stage 5 replaces it with alpha-beta.

## UCI

GUIs start the engine as a separate program and talk to it in lines of text on standard
input and output. A session:

```text
> uci
< id name cb2026 0.1.0
< id author the cb2026 authors
< option name UCI_Chess960 type check default false
< uciok
> isready
< readyok
> position startpos moves e2e4 e7e5
> go wtime 60000 btime 60000 winc 1000 binc 1000
< info depth 1 seldepth 1 score cp 0 nodes 29 time 0 pv g1f3
< bestmove g1f3
```

The GUI owns the game: before every search it sends the whole position, then `go` with the
clocks. Two details matter for robustness:

- **The search runs on its own thread.** The command loop keeps reading, so `isready` is
  answered at once and `stop` takes effect immediately, even mid-search.
- **`go infinite` waits for `stop`** before printing `bestmove`, as the protocol requires,
  even if the engine has already decided.

Bad input is reported (`info string ...`) and never crashes the engine: an invalid FEN, an
illegal move in `position`, an unknown command or option. The protocol tests in
`crates/protocol/src/uci.rs` play scripted sessions covering all of this. That includes
Chess960 castling notation (`e1h1`, "king takes rook") and Windows line endings.

**One binary, three modes.** GUIs send `uci` as their first line, so `cb2026` reads one line
to decide whether to speak UCI, XBoard (stage 20) or open the shell.

## Try it

In the shell:

```text
> play black          # cb2026 plays Black
> e4                  # you move; it replies (randomly, for now)
> play both           # watch it play itself to the end of the game
```

In a GUI such as En Croissant, add `target/release/cb2026` as a UCI engine and play it.

Or by hand:

```text
$ cargo run --release
uci
position startpos moves e2e4
go
```

## What it's worth

A random mover is the zero point of the Elo scale. Every stage from here on is measured
against the one before.

## Further reading

- Chess Programming Wiki: [Zobrist Hashing](https://www.chessprogramming.org/Zobrist_Hashing),
  [Algebraic Chess Notation](https://www.chessprogramming.org/Algebraic_Chess_Notation),
  [UCI](https://www.chessprogramming.org/UCI).
- The UCI protocol description by Stefan Meyer-Kahlen (distributed with most UCI GUIs and
  engines).
