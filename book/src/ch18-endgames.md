# Tablebases, opening books and Chess960

*Stage 18 · tag `stage-18-tablebases` · code in `crates/engine/src/tablebase.rs`,
`crates/syzygy` and `crates/chess/src/position.rs`*

The search and evaluation of the previous chapters are cb2026's own judgement. This chapter
adds knowledge from outside: perfect endgame play from tablebases. It also adds the last part
of Chess960 support: the 960 starting positions themselves. And it adds opening books, for
playing well-known opening moves without searching.

## Endgame tablebases

With seven pieces or fewer on the board, chess is solved. Every such position has been
searched to the end, backwards from every checkmate. The results are stored in **tablebases**.
The **Syzygy** format, by Ronald de Man, is what nearly every engine uses. It comes in two
kinds of file:

- **WDL** tables (win/draw/loss) answer "who wins?". They are compact, and engines probe them
  during the search.
- **DTZ** tables (distance to zeroing) give the number of moves to the next capture or pawn
  move on the best path. That is what a won position needs to make progress under the
  fifty-move rule. Engines use them at the root to choose the move.

Syzygy results take the fifty-move rule into account, which adds two outcomes. A **cursed
win** is a forced mate that takes too long: the fifty-move rule draws it first. A **blessed
loss** is the same thing seen from the other side. cb2026 scores both as draws.

The 3- to 5-piece tables take about 1 GB; all 6-piece tables, about 150 GB. The repository
doesn't include them. `scripts/download-syzygy.sh <directory> 3-4-5` fetches the 5-piece set,
then `setoption name SyzygyPath value <directory>` (or `set syzygy <directory>` in the shell)
loads it.

### Who reads the files

The Syzygy format is an intricate compressed encoding, and a prober is thousands of lines of
careful code with no strength to gain from writing it again. So this is the one place where
cb2026 uses someone else's chess code: the `shakmaty-syzygy` library (GPL-3.0, the prober
behind Lichess's tablebase server). The `docs/PRD.md` section "Tablebase probing" records
that decision.

The search never sees it. The engine crate defines a small trait, and the separate
`cb2026-syzygy` crate implements it:

```rust,ignore
{{#include ../../crates/engine/src/tablebase.rs:params}}
```

The engine crate does no I/O, so it can't open files itself. The program that embeds it (the
`cb2026` binary) hands it a loader function. Tests use a tiny in-memory "tablebase" instead,
which says that king and queen against king is won, and that's enough to test the search
integration without any files.

### Probing in the search

```rust,ignore
{{#include ../../crates/engine/src/search.rs:tb_probe}}
```

Three conditions decide when to probe:

- **Few enough pieces.** At most what the tables cover (and the `SyzygyProbeLimit` option).
- **No castling rights.** The tables don't include them.
- **Right after a capture or pawn move.** A WDL result assumes the fifty-move counter has just
  been reset. Probing then also catches the moment the search crosses into tablebase
  territory, which is the useful moment.

A win scores `TB_WIN − ply`: above every evaluation and below every real mate, so the engine
still prefers an actual checkmate, and a nearer tablebase win over a farther one. The result
goes into the transposition table, and `info ... tbhits` counts the probes that answered.

### At the root

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:tb_root}}
```

When the game itself is in the tables, the DTZ tables pick the move. Searching wouldn't help:
a search sees "won" everywhere and has no reason to make progress, so it can shuffle until the
fifty-move rule draws the game. The tables' move always keeps the best result and heads for
the next zeroing move.

## Chess960

Chess960 (Fischer Random Chess) shuffles the back rank: the bishops on opposite colours, the
king somewhere between the rooks, the black pieces mirroring the white ones. That gives 960
possible starting positions. cb2026 handled Chess960 castling from stage 2 (castling is "king
takes own rook", and the rights are stored as rook files) and Shredder-FEN from stage 1. What
was missing was the starting positions themselves.

They are numbered 0 to 959 (Reinhard Scharnagl's numbering; number 518 is the standard
position), and the number decodes into a back rank one piece at a time:

```rust,ignore
{{#include ../../crates/chess/src/position.rs:chess960}}
```

A test builds all 960 and checks that they are distinct and legal, with the bishops on
opposite colours and the king between the rooks.

- In the shell, `new960 [n]` starts from position `n`, or a random one.
- `match --chess960` plays test matches from the 960 positions and sets `UCI_Chess960` in
  both engines. A first run played Chess960 games with castling to completion, with no
  illegal moves.

## Opening books

At the start of a game the search spends its time rediscovering well-known opening theory,
and it plays the same move every time from the same position. An **opening book** maps
positions to good moves, each with a weight. With `OwnBook` on, the engine looks the position
up before searching and plays a book move at once, chosen at random in proportion to the
weights:

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:book}}
```

cb2026 reads the **Polyglot** format (`.bin` files), which nearly every engine and GUI
supports. A book is a sorted list of 16-byte entries: position key, move, weight and some
learning data the format reserves, all big-endian. Looking a position up is a binary search
for its key:

```rust,ignore
{{#include ../../crates/engine/src/book.rs:moves}}
```

The key is a Zobrist hash (chapter 4), with one difference that matters: the random numbers
are fixed by the format. Any program can then compute the same key for the same position and
read any book. The 781 numbers (768 for pieces on squares, 4 for castling rights, 8 for en
passant files, 1 for the side to move) were published with Fabien Letouzey's PolyGlot program.
cb2026 copies them from python-chess and checks them against the format's published test
hashes:

```rust,ignore
{{#include ../../crates/engine/src/book.rs:key}}
```

One detail is easy to get wrong: the en passant file only counts when a pawn of the side to
move can actually make the capture. cb2026's positions already record en passant squares
that way (chapter 4), for the same reason: equal positions must hash equally.

Polyglot encodes castling as "king takes rook" (`e1h1`), the same convention cb2026 uses
internally, so castling moves need no special treatment.

The book is **off by default**. Tests need to see the engine's own moves, a book would make
every test game start alike, and GUIs and tournament managers usually supply their own books.
In the shell:

```text
> set book /path/to/book.bin
Opening book with 6 entries.
> book
Book moves:
  e4       weight    50  (50%)
  d4       weight    30  (30%)
  Nf3      weight    20  (20%)
The engine ignores the book (set ownbook on to use it).
```

cb2026 ships no book. Any Polyglot book works with it.

## Pitfalls

- **Probing too often.** Every probe reads from disk (or the operating system's file cache).
  `SyzygyProbeDepth` stops probes near the leaves, where they cost more than they save.
- **Shuffling in won endings.** A search that knows "won" but not "how" can draw a won game
  under the fifty-move rule. That's why the root uses the DTZ tables.
- **Castling rights.** A position with castling rights is never in the tables, even with few
  pieces.
- **Chess960 notation.** With `UCI_Chess960` on, castling is sent as king-takes-rook (`e1h1`),
  and GUIs expect it. cb2026 accepts both notations on input.

## What it's worth

| Test | Result |
| --- | --- |
| Without tables or book loaded | No functional change (bench unchanged) |
| With the 3-4-5 piece tables | Measured on the test Mac (`SyzygyPath` set for one engine) |

## Further reading

- Chess Programming Wiki: [Syzygy Bases](https://www.chessprogramming.org/Syzygy_Bases),
  [Endgame Tablebases](https://www.chessprogramming.org/Endgame_Tablebases),
  [Chess960](https://www.chessprogramming.org/Chess960),
  [Opening Book](https://www.chessprogramming.org/Opening_Book),
  [PolyGlot](https://www.chessprogramming.org/PolyGlot).
- [syzygy-tables.info](https://syzygy-tables.info/), to look up any tablebase position.
