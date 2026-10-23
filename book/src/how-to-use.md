# How to use this book

## Reading the book locally

```sh
cargo install mdbook
mdbook serve book --open
```

## Running the engine at any stage

```sh
git clone https://github.com/cjus/chessengine
cd chessengine
git checkout stage-05-alphabeta   # any stage tag
cargo build --release
./target/release/cb2026
```

The Rust toolchain version is pinned in `rust-toolchain.toml`; `rustup` installs it on first
use.

## Code excerpts

The code in each chapter is included straight from the source files, so it always shows the
current code. Functions grow as later stages add to them: chapter 5's negamax, read today,
already has the transposition table from chapter 8 in it. To see the code exactly as a chapter
first described it, check out that chapter's stage tag.

## Switching techniques off

From the stages that add search techniques onwards, every technique can be switched off at
runtime (in the shell with `set <technique> off`), so you can measure what each one is worth
yourself. `techniques` lists them all, and `ab <technique> [depth]` does the measuring for
you: it searches the current position with the technique on and off and compares nodes, time
and the move chosen.

```text
> ab nullmove 7
Null-move pruning (nullmove), depth 7, one thread, empty hash table:
              nodes      time  move      score
  on           9088    0.00 s  d6        -0.08
  off         13402    0.01 s  d6        -0.08
Switched off, the search needs 1.47× the nodes.
```

One position says little; try several (and `bench`, which searches a fixed set).

## Looking inside the search

The shell has commands for watching the search decide:

- **`stats`** after a search: how often the transposition table answered, how often the first
  move searched caused the cutoff (a direct measure of move ordering), how often each pruning
  technique fired, and how often late move reductions had to search again.
- **`tree <depth> [plies]`** prints the first plies of a search tree: each node's window and
  score, the cutoffs, and the moves pruned or reduced, with the reason. Start small
  (`tree 4 2`): trees grow fast.
- **`explain <move>`** searches a move and compares it with the engine's choice, showing the
  engine's answer to it.
- **`eval`** breaks the evaluation into its terms; **`eval diff <move>`** shows which terms a
  move changes.
- **`see <move>`** shows the static exchange evaluation of a capture.

```text
> tree 4 2
Na6  depth 3 [+0.32, +0.33] (beta cutoff by Na3 (move 1)) -> +0.55
  (pass)  depth 0 [-0.33, -0.32] -> -0.18
  Na3  depth 2 [-0.33, -0.32] -> -0.55
    b5  pruned (late move pruning)
    ...
```

Read that as (in the position after 1. d3): after 1...Na6, the null move (`(pass)`) didn't
cut off, but White's first real move, Na3, did. Below it, late move pruning skipped most of
Black's quiet replies.

## Quick matches

`match <engine> <games> [time control]` plays cb2026, exactly as you've set it up in the shell
(level, techniques switched off, hash size), against another UCI engine: an older stage's
binary, or any engine you have. `selfplay <games>` plays it against a copy of cb2026 at
default settings, which is a quick way to see what switching a technique off costs in games:

```text
> set nullmove off
> selfplay 6 nodes=20000
...
Match over: cb2026 1.0.0 (shell) scored 1.5/6 against cb2026 1.0.0 (default) (+1 -4 =1).
Elo difference: -191 ± 1219 (95%). A few games say little: the margin shows how little.
```

Six games, or even a hundred, can't tell a 30 Elo change from noise; the `match` tool's SPRT
(see `docs/testing.md`) decides when enough games have been played. Games are saved as PGN
(`cb2026-match.pgn` unless you give `pgn <file>`).
