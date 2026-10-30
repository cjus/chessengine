# cb2026

**cb2026** (ChessBrain 2026) is a chess engine in Rust, written from scratch to be both strong
and easy to learn from. It speaks UCI and XBoard, so it plays and analyses in chess GUIs such as
En Croissant and Cute Chess. It comes with an interactive shell for playing games, watching the
engine think and switching its techniques on and off. It also speaks the
[Chess Node Protocol](https://github.com/cjus/chess-node-protocol) (CNP), the JSON protocol
through which Solrac, a 3D chess board that teaches, reaches it. A companion book, *Building a
Chess Engine in Rust* (in `book/`), explains every technique as the engine gained it.

**Strength:** about **3379 ± 21** on the CCRL Blitz scale (2026-10-29) for `main`, from 5,600
games at 8+0.08 against seven rated engines, with the third NNUE network it trained on its own
games. v1.0.0, with the second, rated **3286 ± 17**; with the first it rated 3236 ± 16, and with
its hand-crafted evaluation 2782 ± 11. The weaker reference engines say little at this level,
so the figures rest mostly on the two strongest.

## Download

Each [release](https://github.com/cjus/chessengine/releases) has a package for each platform,
holding the `cb2026` program, this README, the licences and the third-party notices:

| Package | For |
| --- | --- |
| `cb2026-<version>-aarch64-apple-darwin.tar.gz` | Macs with Apple silicon (M1 and later) |
| `cb2026-<version>-x86_64-apple-darwin.tar.gz` | Intel Macs |
| `cb2026-<version>-x86_64-pc-windows-msvc.zip` | Windows, x86-64 |
| `cb2026-<version>-x86_64-unknown-linux-gnu.tar.gz` | Linux, x86-64 |

The network is built into the program, so no other file has to sit beside it. The release also
carries the network itself (`cb2026-gen3-256.nnue` from v1.1; v1.0.0 carries gen2's), an opening book made from
Lichess's games (`cb2026-lichess-2026-09.bin`, below) and `SHA256SUMS.txt`. To check what you
downloaded, run `shasum -a 256 -c SHA256SUMS.txt --ignore-missing` in the folder holding the
files.

The Mac programs are signed with a Developer ID and notarised by Apple. The first time one runs,
macOS checks with Apple online; after that it opens with no warning. The Windows program is not
signed, so Windows may warn about it the first time ("More info", then "Run anyway").

## In a chess GUI

Add `cb2026` (`cb2026.exe` on Windows) as a UCI engine. It's been checked in **En Croissant**,
**Cute Chess**, **XBoard** (as an XBoard engine) and **HIARCS Chess Explorer**
([`docs/gui-compat.md`](docs/gui-compat.md)); other UCI and XBoard GUIs should work too. Its
options:

| Option | Default | What it does |
| --- | --- | --- |
| `Threads` | 1 | Search threads (Lazy SMP), up to 256 |
| `Hash` | 16 | Transposition table, in MB (up to 65,536) |
| `MultiPV` | 1 | How many best lines to search and show |
| `Ponder` | off | Think on the opponent's time |
| `EvalFile` | `<built-in>` | The NNUE network: `<built-in>`, a path to another `.nnue` file, or `none` for the hand-crafted evaluation |
| `OwnBook`, `BookFile` | off, empty | Play from a Polyglot opening book (`.bin`) |
| `SyzygyPath` | empty | A folder of Syzygy endgame tablebases |
| `SyzygyProbeDepth`, `SyzygyProbeLimit` | 1, 7 | From what depth, and with how many men at most, the search probes them |
| `Skill Level` | 20 | Strength level 0–20; 20 is full strength |
| `UCI_LimitStrength`, `UCI_Elo` | off, 2500 | Play at about this rating, 600–2500 |
| `UCI_Chess960` | off | Chess960 (Fischer random) |
| `Move Overhead` | 30 | Milliseconds kept back on each move for the GUI's and the system's delays |

**The opening book.** By default cb2026 thinks from the first move. To play from the book in
the release, set `BookFile` to the path of `cb2026-lichess-2026-09.bin` and turn on `OwnBook`.
The book is in the Polyglot format, made from Lichess's September 2026 games between players
rated 2200 or more, at blitz or slower (CC0): 97,432 moves for the first 20 plies. `scripts/lichess-book.sh` makes
one from any month.

**Tablebases.** With `SyzygyPath` set, cb2026 looks up endings in the tables as it searches,
and at the root plays the tables' move, so it never spoils a won or drawn ending the tables
cover (up to seven men, as many as you have). `scripts/download-syzygy.sh <folder> 3-4-5`
fetches the 3-, 4- and 5-man tables (about 1 GB).

**Strength levels.** `Skill Level` below 20, or `UCI_LimitStrength` with `UCI_Elo`, makes
cb2026 play like a person: fewer nodes, and on some moves a person's short horizon, so it
blunders about as often as Lichess rapid players of that rating do. The ratings are estimated
from blunders, not measured against people (book chapter 19). The levels play with the
hand-crafted evaluation, which they were calibrated with.

## In a terminal

```
cb2026              # the interactive shell; `help` lists its commands
cb2026 --uci        # UCI mode (a GUI switches to it by sending "uci")
cb2026 --xboard     # XBoard/CECP mode
cb2026 --cnp        # Chess Node Protocol mode (Solrac switches to it by sending JSON)
cb2026 bench        # the fixed benchmark: total nodes and speed
cb2026 bench none   # the same with the hand-crafted evaluation
```

In the shell you can play a game against the engine (`play black`, then `e4`, or `time 5+3`
for clocks), ask for a `hint`, see its thinking depth by depth, look up the book and the
tablebases, and switch its techniques on and off (`set nullmove off`, `set evalfile none`) to
see what each one does. To run it as a Lichess bot, see [`docs/lichess.md`](docs/lichess.md).

## Learning from it

The book in `book/`, *Building a Chess Engine in Rust*, has a chapter for each stage, from the
board and move generation to the NNUE network. Each chapter gives the idea, the real code and
what it measured. Build it with [mdBook](https://rust-lang.github.io/mdBook/): `mdbook build
book`. Every technique can be switched off, and [`docs/elo-ledger.md`](docs/elo-ledger.md)
records what each one gained.

## Building from source

```
cargo build --release
```

The Rust version is pinned in `rust-toolchain.toml`. The build fetches the Chess Node Protocol
crate from its repository, which needs access to it while that repository is private.
`RUSTFLAGS="-C target-cpu=native"` makes a build a little faster on the machine that built it.
The release packages are built for each platform's baseline CPU, so they run everywhere.
Releases are made from version tags: see [`docs/releasing.md`](docs/releasing.md).

## License

cb2026, its book and its tools are licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

The one exception is Syzygy tablebase probing. The `cb2026-syzygy` crate links
[`shakmaty-syzygy`](https://crates.io/crates/shakmaty-syzygy), which is GPL-3.0-or-later, so a
`cb2026` binary built with the `syzygy` feature (on by default, and in the released binaries)
is GPL-3.0-or-later as a whole: see
[`crates/syzygy/LICENSE-GPL-3.0`](crates/syzygy/LICENSE-GPL-3.0). Built with
`cargo build --release --no-default-features`, cb2026 has no Syzygy probing and is
`MIT OR Apache-2.0` throughout.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
