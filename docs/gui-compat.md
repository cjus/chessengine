# GUI compatibility matrix

Whether cb2026 works in the GUIs people use, re-checked at each release (see `docs/PRD.md`,
"Compatibility matrix"). GUI checks happen on Carlos's M1 Max; Windows checks on his Windows
machine once the Windows build is ready (cjus/chessengine#11).

## Automated checks (every pull request)

Claude can't operate a GUI, so a **conformance test** plays scripted sessions against the real
binary the way GUIs do, including their awkward habits, and checks every response:
`crates/cli/tests/conformance.rs`, run by `cargo test` (in `scripts/check.sh` and nightly CI).

| Habit | Test |
| --- | --- |
| Windows line endings (`\r\n`) | `uci_handshake_with_windows_line_endings`, `xboard_force_setboard_and_windows_line_endings` |
| Options changed while the engine thinks | `uci_options_sent_mid_search` |
| `position` without `ucinewgame` | `uci_position_without_ucinewgame` |
| Very short clocks (50 ms) | `uci_very_short_clock` |
| `stop` straight after `go` | `uci_stop_straight_after_go` |
| A position with no legal move | `uci_game_over_position` |
| A string option cleared with an empty value (`setoption name BookFile value`) | `uci_string_option_cleared_with_an_empty_value` |
| An XBoard game with clock, pondering and thinking output | `xboard_game_with_clock` |

The protocol crates' unit tests cover the rest of each protocol (pondering, MultiPV,
Chess960, every option).

## Manual checks (each release)

How to run each check:

| Check | What to do | Expected |
| --- | --- | --- |
| **Load** | Add `target/release/cb2026` as an engine | It's recognised as a UCI engine (XBoard engine in XBoard), named `cb2026 <version>` |
| **Options** | Open the engine's options dialog | Hash, Threads, Ponder, MultiPV, Skill Level, UCI_LimitStrength/UCI_Elo, SyzygyPath, OwnBook/BookFile, EvalFile, Move Overhead are listed and can be changed |
| **Analysis + MultiPV** | Analyse a position with 3 lines | Three lines, each with depth, score and PV, updating as it thinks; switching positions restarts the analysis |
| **Timed game** | Play a 3+2 game against it | It moves within its time and never loses on time; the game ends normally |
| **Chess960** | Start a Chess960 game, castle with both sides | Castling works for both sides; no illegal-move errors |
| **Pondering** | Turn pondering on, play a game | It thinks during your turn (the GUI shows ponder output) and moves fast when you play the expected move |
| **Add / remove** | Remove the engine and add it again | No errors; settings are kept or reset as the GUI does for other engines |

## Status

`✓` works, `✗` fails (link the issue), `–` not applicable, blank: not checked yet.

### Release 1.0

Checked by Carlos on the M1 Max by 2026-10-23 (cjus/chessengine#37), with
`cb2026 1.0.0` built from the version branch (cjus/chessengine#154). The branch was rebuilt as
fixes landed; its last build, at `cb2170b` (bench 749,181), had both fixes below and
Stockfish 17's cutoff reductions (#167), a search change only. Earlier rows were checked on
builds without some of these, and only the cells a fix touched were checked again.

| GUI | Version | Platform | Load | Options | Analysis + MultiPV | Timed game | Chess960 | Pondering | Add / remove |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| En Croissant | 0.15.1 | macOS | ✓ | ✓ | ✓ | ✓ | ✓ | – | ✓ |
| Cute Chess | 1.5.1 (built from source, Qt 6.12.0) | macOS | ✓ | ✓ | – | ✓ | ✓ | ✓ | ✓ |
| Scid vs. PC | | macOS | | | | | | | |
| XBoard (CECP mode, `cb2026 --xboard`) | 4.9.1 (Homebrew, XQuartz) | macOS | ✓ | ✓ | ✓ (after #170) | ✓ | ✓ | ✓ | ✓ |
| HIARCS Chess Explorer Pro | 1.5.2 | macOS | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| ChessBase | | Windows | | | | | | | |
| lichess-bot (`docs/lichess.md`) | | macOS | | – | – | | | | – |

The timed games were 3+2, with no illegal moves or time losses. Not checked for 1.0: Scid vs.
PC; ChessBase, which waits for the Windows build (cjus/chessengine#11); and lichess-bot, whose
games follow the release (cjus/chessengine#191; Carlos, 2026-10-23).

`EvalFile`, the option for an NNUE network, came after these checks (#185). It is a string
option like `BookFile` and `SyzygyPath`, which every GUI here showed, and the protocol tests
cover it. Its default is `<built-in>`, the network built into the engine (#196); a GUI that
clears the field (an empty value) gets the hand-crafted evaluation.

**Found and fixed during the checks:**
- **An empty string option** (`setoption name BookFile value`, which is how GUIs clear a path)
  was taken for an unknown option, so the old book or tablebases stayed in use. Found in En
  Croissant; fixed in #169.
- **MultiPV in XBoard mode** showed one line per depth, because the XBoard adapter sent only
  the first. Since #170 it sends every line, with each depth's best last: XBoard sorts the lines
  by score to show them, but takes the last one as the engine's score and PV for the PGN, the
  evaluation graph and adjudication.

**Notes by GUI, from these checks:**

- **En Croissant** (0.15.1):
  - The version is in the engine's name. Its Version field stays empty for an engine added from
    a file: it is filled only for engines from its own download list, and UCI has no version
    field.
  - It never sends `go ponder`, so pondering is "–".
  - It plays Chess960 only from a Chess960 starting position, and sets `UCI_Chess960` itself.
    Set one up with the board's Edit position button and the "Enter FEN" box before starting.
    It was checked from `rnbkqbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBKQBNR w KQkq - 0 1`, which En
    Croissant names Fischer Random 534.
  - Its engine page lists every option except the button (Clear Hash). SyzygyPath gets a folder
    picker and BookFile a file box.
- **Cute Chess** (1.5.1):
  - It has no release for macOS, so it was built from its v1.5.1 tag.
  - It names an engine after its file (`cb2026`), not its `id name`.
  - It has no analysis mode, so that is "–". Chess960 is its variant `fischerandom`, and
    pondering is "Thinking on opponent's time" in the game settings.
  - In dark mode its Add (+) button is nearly invisible.
- **XBoard** (4.9.1):
  - Its title shows `cb2026 1.0.0`.
  - Only Skill Level and MultiPV are engine options in its protocol. Hash, CPUs and the
    tablebase path are under Engine → Common Settings.
  - Pondering is Options → General Options → Ponder Next Move.
  - An engine started with `-fcp` isn't added to Engine → Engine List. It is added through
    Engine → Load New 1st Engine, with "Add this engine to the list" ticked.
- **HIARCS Chess Explorer Pro** (1.5.2):
  - It shows `cb2026 1.0.0`.
  - Analysis is View → Analysis Explorer, with a "lines" field for MultiPV.
  - Games are under Play → New game…, with the time control, "Think on player's time" and
    Chess960 (Starting Position).

## GUI notes

- **XBoard:** start the engine with `xboard -fcp "/path/to/cb2026 --xboard"` (or with no
  flag: cb2026 detects the protocol from the GUI's first line). For Chess960, choose the
  variant *fischerandom*; castling is entered and shown as `O-O` / `O-O-O`.
- **Cute Chess and En Croissant** send `setoption` for every option at startup; that's
  expected.
- **HIARCS Chess Explorer** and **ChessBase** load UCI engines as "external" or "UCI"
  engines; point them at the binary itself.
