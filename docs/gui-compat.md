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

### Release 1.0 (stage 20)

| GUI | Platform | Load | Options | Analysis + MultiPV | Timed game | Chess960 | Pondering | Add / remove |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| En Croissant | macOS | | | | | | | |
| Cute Chess | macOS | | | – | | | | |
| Scid vs. PC | macOS | | | | | | | |
| XBoard (CECP mode, `cb2026 --xboard`) | macOS | | | | | | | |
| HIARCS Chess Explorer | macOS | | | | | | | |
| ChessBase | Windows | | | | | | | |
| lichess-bot (`docs/lichess.md`) | macOS | | – | – | | | | – |

Notes from the checks go here, with the GUI's version.

## GUI notes

- **XBoard:** start the engine with `xboard -fcp "/path/to/cb2026 --xboard"` (or with no
  flag: cb2026 detects the protocol from the GUI's first line). For Chess960, choose the
  variant *fischerandom*; castling is entered and shown as `O-O` / `O-O-O`.
- **Cute Chess and En Croissant** send `setoption` for every option at startup; that's
  expected.
- **HIARCS Chess Explorer** and **ChessBase** load UCI engines as "external" or "UCI"
  engines; point them at the binary itself.
