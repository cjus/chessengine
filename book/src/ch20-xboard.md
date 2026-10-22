# XBoard, pondering and playing on Lichess

*Stage 20 · tag `stage-20-xboard` · code in `crates/protocol/src/xboard.rs`, `uci.rs` and
`crates/cli/tests/conformance.rs`*

This is the last stage of the hand-crafted engine. Nothing here makes cb2026 search better;
it makes the engine work in more places. It adds a second GUI protocol, thinking on the
opponent's time, a test suite that plays the part of the GUIs, and a setup for playing people
online.

## Two protocols, two ideas of who keeps the game

Chapter 4 introduced UCI, where the GUI owns the game. Before every move it sends the whole
position (`position startpos moves e2e4 e7e5 ...`) and says `go`. The engine remembers nothing
it needs between moves; it's a function from position to move.

**XBoard**, also called **CECP** (the Chess Engine Communication Protocol), is older (it grew
out of the GNU Chess interface in the early 1990s) and works the other way round. **The engine
keeps the game.** The GUI sends one move at a time, and the engine decides by itself when it's
its turn:

```text
> new                 start a game; the engine plays Black
> level 40 5 0        40 moves in 5 minutes
> post                show thinking
> time 30000          the engine's clock: 300.00 seconds (centiseconds)
> otim 30000          the opponent's clock
> usermove e2e4       the opponent's move: now it's the engine's turn
< 1 -15 0 21 e7e5     thinking: depth, score, time, nodes, PV
< ...
< move e7e5
```

So an XBoard engine is a small state machine. It holds the position and the moves that led to
it (for `undo` and `remove`, which take back one or two plies). It also tracks which side it
plays: `new` makes it Black, `go` makes it play the side to move and start thinking, and
`force` makes it play neither, so the GUI can enter moves from a game. Finally it holds the
clock settings: `level`, `st` (seconds per move), `sd` (maximum depth), and the clock readings
`time`/`otim` sent before each move.

cb2026 keeps all of that in one `Session`, and turns it into the same `Limits` the UCI side
builds, so both protocols drive exactly the same search through the Engine API.

### Thinking output

With `post`, every completed iteration prints a line in the format XBoard expects:

```rust,ignore
{{#include ../../crates/protocol/src/xboard.rs:thinking}}
```

Scores are in centipawns from the engine's side. Mates use a convention: 100000 plus the
number of moves to mate, negative when being mated, so a GUI can show "#3".

### Feature negotiation

XBoard learns what an engine supports from `feature` lines sent in answer to `protover 2`.
Some features change the protocol itself:

- `usermove=1`: moves arrive as `usermove e2e4`, not as bare `e2e4`, so they can't be confused
  with commands.
- `setboard=1`: positions arrive as FEN, not through the old `edit` mode.
- `san=0`: moves are in coordinate notation.
- `ping=1`: `ping N` is answered with `pong N`, so the GUI can tell when the engine has caught
  up.

Others announce abilities: `analyze`, `memory` (hash size), `smp` (threads), `egt="syzygy"`
(tablebases) and `variants="normal,fischerandom"` (Chess960). In Chess960, XBoard writes
castling as `O-O` and `O-O-O`, because "king to g1" is ambiguous when the king may start on g1.

### Pitfalls

- **The race after `move`.** As soon as the engine prints `move e7e5`, the GUI may answer with
  the next move. If the engine's own board hasn't played e7e5 yet, the reply looks illegal. The
  search thread records the move where the session can see it *before* printing it, and the
  session plays any recorded moves before it handles the next command.
- **Commands while thinking.** `force`, `new`, `undo` or `result` can arrive mid-search. The
  search is stopped without moving: the GUI no longer wants a move. `?` is different. It means
  "move now", so the search stops and plays its best move so far.
- **Clocks in centiseconds**, `level` times in minutes or `minutes:seconds`, and `st` in seconds.
  Three units for one idea.

## Pondering

A chess clock only runs on your own turn, but nothing stops a program from thinking during the
opponent's. **Pondering** guesses the opponent's reply (the second move of the principal
variation) and searches the position after it while the opponent thinks. If the guess is right
(a **ponder hit**), that search has a head start, often of several plies. If it's wrong, it's
thrown away, and nothing is lost but electricity.

Strong engines guess right about half the time, so pondering is worth real strength in games
against opponents who think. It's no use in self-play tests (both engines would share the
cores), which is why the `match` tool leaves it off.

### In UCI

The GUI manages pondering. After `bestmove e7e5 ponder g1f3` it sends
`position ... e7e5 g1f3` and `go ponder wtime ...`. The engine searches as if with no limit
until either `ponderhit` (the opponent played g1f3: switch to the real clock) or `stop` (they
didn't: report a move, which the GUI ignores, and wait for a new `position`):

```rust,ignore
{{#include ../../crates/protocol/src/uci.rs:ponder}}
```

On `ponderhit` a timer starts with the time the search would have been given for this move.
The pondering time is a bonus on top, not counted against it.

### In XBoard

XBoard leaves pondering to the engine: `hard` turns it on, `easy` off. After printing its
move, cb2026's search thread carries on by itself. It plays the expected reply on its own
board and searches with no limit:

```rust,ignore
{{#include ../../crates/protocol/src/xboard.rs:ponder}}
```

When the opponent's `usermove` arrives, the session compares it with the expected move. A hit
clears the ponder flag and starts the timer, as in UCI, and the same search goes on to produce
the next move. A miss stops the thread without a move and starts a fresh search. Each search
gets its own stop flag, so a timer left over from one search can never stop the next.

## Conformance tests: standing in for the GUIs

Every GUI drives engines a little differently, and the differences are where engines break.
GUIs send Windows line endings, change options mid-search, skip `ucinewgame`, give a few
milliseconds on the clock, or send `stop` a microsecond after `go`. Claude, who writes most of
this code, can't operate a GUI. So `crates/cli/tests/conformance.rs` starts the real `cb2026`
binary, plays scripted sessions with exactly those habits, and checks every answer. It runs
with the other tests on every change. The manual checklist in `docs/gui-compat.md` covers what a script can't:
how the engine looks and behaves inside each GUI.

## Playing on Lichess

Lichess lets programs play as BOT accounts through its Bot API.
[lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) is the usual bridge: it accepts
challenges and talks UCI to the engine. `docs/lichess.md` has the setup: the BOT account, the
config, and the settings that matter. Two of those settings are worth explaining here:

- **Move overhead.** The clock runs on Lichess's server. A move that the engine makes with
  50 ms to spare can arrive after its time has run out, so the engine keeps a margin per move
  (the `Move Overhead` option, 300 ms suggested online).
- **One game at a time,** with all threads. Two games at once would split the cores and make
  both weaker.

## What it's worth

At full strength the search is unchanged (the bench is unchanged). Pondering's value depends
on the opponent and the time control; it isn't measured in the self-play tests, which run
without it. The GUI compatibility matrix is checked on the test Mac before the 1.0 release.

## Further reading

- Tim Mann and H.G. Muller, [Chess Engine Communication Protocol](https://www.gnu.org/software/xboard/engine-intf.html),
  the XBoard specification.
- Chess Programming Wiki: [Chess Engine Communication Protocol](https://www.chessprogramming.org/Chess_Engine_Communication_Protocol),
  [Pondering](https://www.chessprogramming.org/Pondering).
- [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) and the
  [Lichess Bot API](https://lichess.org/api#tag/Bot).
