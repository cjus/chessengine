# Running cb2026 as a Lichess bot

[Lichess](https://lichess.org) lets programs play as **BOT accounts**, through its
[Bot API](https://lichess.org/api#tag/Bot). The usual way to connect an engine is
[lichess-bot](https://github.com/lichess-bot-devs/lichess-bot), an open-source (AGPL-3.0) Python
bridge: it accepts challenges, starts the engine, and relays moves between Lichess and the
engine over UCI. lichess-bot is a separate project; cb2026 doesn't bundle it (see
`docs/PRD.md`, "Lichess bot").

This page says how to set it up and which settings suit cb2026. Running the bot is Carlos's
job (see `docs/PRD.md`, "Compatibility matrix"): it needs a Lichess account and a machine
that stays on.

## 1. A BOT account

1. Create a **new** Lichess account for the bot (for example `cb2026-bot`). It must not have
   played any games: an account that has played can't become a bot. Don't use your own account.
2. While logged in as the bot, create an API token at
   <https://lichess.org/account/oauth/token> with the scope **"Play games with the bot API"**
   (`bot:play`). Keep it secret: anyone with it can play as the bot.
3. The account is upgraded to a BOT account by lichess-bot itself the first time (step 3
   below, with `-u`). The upgrade can't be undone.

BOT accounts can only play other bots and humans who challenge them; they can't join pools or
tournaments for humans. Lichess rates them like any other account, on the Lichess scale. The
levels' ratings are estimates on that scale, from how often people blunder (`docs/PRD.md`,
"Levels and strength limiting"), so a bot playing people at a fixed level would check them.

## 2. Build the engine

```sh
cargo build --release          # target/release/cb2026
```

Optional extras for stronger play:

- **Syzygy tablebases:** `scripts/download-syzygy.sh <dir> 3-4-5` fetches the 3–5 piece tables
  (about 1 GB). Pass the directory as `SyzygyPath` below.
- **Opening book:** any Polyglot `.bin` book, through lichess-bot's own book support or
  cb2026's `OwnBook` + `BookFile` options (use one, not both).

## 3. Install and configure lichess-bot

Follow lichess-bot's README to install it (Python 3 and a virtual environment). Then copy its
`config.yml.default` to `config.yml` and set the parts below. The key names here follow
lichess-bot's default config; they change from time to time, so compare with the
`config.yml.default` of the version you installed.

```yaml
token: "<the token from step 1>"

engine:
  dir: "/path/to/chessengine/target/release/"
  name: "cb2026"
  protocol: "uci"
  ponder: true                 # think on the opponent's time
  uci_options:
    Threads: 4                 # the machine's performance cores, minus one for the system
    Hash: 256                  # MB
    Move Overhead: 300         # ms reserved per move for network lag
    SyzygyPath: "/path/to/syzygy"   # leave out if you have no tables
    # For a weaker, friendlier bot, use a level instead of full strength:
    # Skill Level: 10
    # or UCI_LimitStrength: true and UCI_Elo: 1600

challenge:
  concurrency: 1               # one game at a time: Threads are for that game
  accept_bot: true
  variants:
    - standard
    - chess960
  time_controls:
    - blitz
    - rapid
    - classical
  modes:
    - casual
    - rated
  min_base: 60                 # seconds; at least 1 minute
  min_increment: 0
```

Then upgrade the account and start the bot:

```sh
python3 lichess-bot.py -u      # first time only: turns the account into a BOT account
python3 lichess-bot.py
```

## Recommended settings, and why

| Setting | Value | Why |
| --- | --- | --- |
| Protocol | UCI | cb2026 also speaks XBoard, but lichess-bot's UCI support is the most used and tested |
| `Move Overhead` | 300 ms | The clock runs on Lichess's server, and each move has to travel there. Without a margin the engine can lose on time with time still showing on its own clock |
| Time controls | blitz, rapid, classical (1 minute and up) | cb2026's time management is tested at 8+0.08 and longer; in bullet, network lag is a large part of the clock |
| `concurrency` | 1 | Each game uses all the threads; two games at once would make both weaker |
| `ponder` | true | Free strength: the engine thinks on the opponent's time (see book chapter 20) |
| Variants | standard, chess960 | Both are supported; cb2026 doesn't play other variants |
| `Threads` | performance cores minus one | Leaves a core for lichess-bot and the system |

## Checking it works

1. Start the bot and watch its log: it should print the engine's name (`cb2026 ...`) and
   "You're now connected to https://lichess.org/".
2. From your own account, challenge the bot to a casual 5+3 game, and a casual Chess960 game.
3. Check that it moves within its time, ponders (the log shows `go ponder` after its moves),
   and that the game ends normally (resign, mate or draw).

If it loses on time, raise `Move Overhead`. If it doesn't start, run
`target/release/cb2026 --uci` by hand and type `uci`: it should answer with its options and
`uciok`.

## Further reading

- [lichess-bot wiki](https://github.com/lichess-bot-devs/lichess-bot/wiki): configuration,
  opening books, and running it as a service.
- [Lichess Bot API](https://lichess.org/api#tag/Bot) and the
  [bot rules](https://lichess.org/page/bots) (no human may play through a bot account, and the
  account must be marked as a bot).
