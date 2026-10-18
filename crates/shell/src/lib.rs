//! # cb2026-shell: the interactive shell
//!
//! Running `cb2026` in a terminal opens a command shell for playing and studying chess with the
//! engine. It is a front end like a GUI, but it can also show the engine's internals.
//!
//! The shell is split in two so it can be tested without a terminal:
//!
//! - [`Shell::execute`] runs one command line and returns its output as a string.
//! - [`run`] reads lines from any input, executes them and writes the results, adding a prompt
//!   when a person is typing.
//!
//! ## Commands so far
//!
//! | Command | What it does |
//! | --- | --- |
//! | `new`, `startpos` | Set up the starting position |
//! | `fen [FEN]` | Set a position from FEN, or print the current FEN |
//! | `show`, `d` | Draw the board |
//! | `flip` | Draw the board from the other side |
//! | `board unicode\|ascii` | Chess symbols or letters |
//! | `color on\|off` | Coloured squares |
//! | `e4`, `Nf3`, `e2e4`, `move e4 e5 Nf3 ...` | Play moves (algebraic or UCI notation) |
//! | `undo`, `redo` | Take back the last move; play it again |
//! | `time 5+3`, `time off` | Play with clocks; the engine plays to its clock |
//! | `hint`, `resign`, `draw` | The engine's suggestion; resign; offer a draw |
//! | `pgn`, `pgn save\|load <file>` | The game as PGN; save it or load one |
//! | `go [depth N\|nodes N\|movetime MS]` | The engine moves now and explains its choice |
//! | `play white\|black\|both\|none` | Which sides the engine plays |
//! | `legal`, `moves` | List the legal moves |
//! | `perft <depth>`, `divide <depth>` | Count positions (move generation check) |
//! | `attacks <square>`, `attackers <square>` | What a piece attacks; what attacks a square |
//! | `magic <square>` | How the magic-bitboard lookup works out a slider's attacks there |
//! | `zobrist` | The position's hash key and the parts it is built from |
//! | `set sliders rays\|magics` | Compute sliding attacks by walking rays or by magic lookup |
//! | `set depth <N>`, `set movetime <MS>` | How long the engine thinks about its own moves |
//! | `think on\|off` | Show the engine's thinking depth by depth |
//! | `set alphabeta on\|off` | Switch alpha-beta off to compare with plain minimax |
//! | `set quiescence on\|off`, `set mvvlva on\|off`, `set tt on\|off` | Switch the quiescence search, MVV-LVA move ordering or the transposition table off |
//! | `set hash <MB>` | Transposition table size |
//! | `set threads <N>` | Search threads (Lazy SMP) |
//! | `new960 [n]` | Chess960 starting position `n` (0–959; random if left out) |
//! | `level <0-20>`, `elo <n>\|off`, `set multipv <N>` | Strength level; limit to an Elo; show several lines |
//! | `set syzygy <path>`, `tb` | Load Syzygy tablebases; show the tablebase result for the position |
//! | `set book <path>`, `set ownbook on\|off`, `book` | Load a Polyglot opening book; play from it; show its moves here |
//! | `set killers\|countermoves\|history\|conthist on\|off` | Switch a quiet-move ordering heuristic off |
//! | `set pvs on\|off`, `set aspiration on\|off` | Principal variation search; aspiration windows |
//! | `set nullmove on\|off` | Null-move pruning |
//! | `set lmr on\|off`, `set histlmr on\|off`, `set lmp on\|off` | Late move reductions; adjusted by history; late move pruning |
//! | `set rfp\|razoring\|futility\|seeprune\|delta\|seeorder on\|off` | Stage 14's pruning and SEE ordering |
//! | `eval` | The static evaluation, term by term |
//! | `eval diff <move>`, `see <move>` | Which evaluation terms a move changes; its static exchange evaluation |
//! | `explain <move>` | Search a move and compare it with the engine's choice |
//! | `stats`, `tree <depth> [plies]` | What the last search did; the search tree's first plies |
//! | `techniques`, `ab <name> [depth]` | Every switch and its state; search with and without one and compare |
//! | `set pst on\|off`, `set tapered on\|off` | Switch the piece-square tables or the middlegame/endgame blend off |
//! | `set mobility\|pawns\|pieces\|kingsafety\|tempo on\|off` | Switch a group of evaluation terms off |
//! | `bench` | The fixed benchmark |
//! | `bb <set>` | Show a bitboard: `occupied`, `empty`, `white`, `black`, `checkers`, `pinned`, or a piece letter (`P`, `n`, ...) |
//! | `match <engine> <games> [tc]`, `selfplay <games> [tc]` | A short match against another UCI engine, or against a copy of cb2026 |
//! | `uci`, `xboard` | Switch the session to a protocol |
//! | `help` | List commands |
//! | `quit`, `exit` | Leave |
//!
//! The full plan is in `docs/PRD.md`, "The interactive shell".

#![forbid(unsafe_code)]

mod explain;
mod game;
pub mod render;
mod versus;

use std::io::{self, BufRead, Write};

use std::time::Instant;

use cb2026_chess::attacks::{
    SliderMethod, bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks,
    rook_attacks, set_slider_method, slider_method,
};
use cb2026_chess::game::Game;
use cb2026_chess::magic::{self, Magic};
use cb2026_chess::perft::{divide, perft};
use cb2026_chess::zobrist;
use std::sync::atomic::AtomicBool;

use cb2026_chess::{Bitboard, Color, Move, Piece, PieceType, Position, Square};
use std::time::Duration;

use cb2026_engine::book::OpeningBook;
use cb2026_engine::levels;
use cb2026_engine::stats::SearchStats;
use cb2026_engine::tablebase::{self, TbWdl};
use cb2026_engine::tt::MAX_HASH_MB;
use cb2026_engine::{Engine, Limits, MAX_THREADS, NAME, Score, SearchInfo, bench, eval, search};
pub use cb2026_protocol::Protocol;
use game::Clock;
pub use render::Style;

/// The help text for the `help` command.
const HELP: &str = "\
Commands:
  new, startpos        Set up the starting position
  fen [FEN]            Set a position from FEN, or print the current FEN
  show, d              Draw the board
  flip                 Draw the board from the other side
  board unicode|ascii  Chess symbols or letters
  color on|off         Coloured squares
  e4, Nf3, O-O, e2e4   Play a move (algebraic or UCI notation)
  move <m1> <m2> ...   Play several moves
  undo, redo           Take back the last move; play it again
  time <min>+<sec>     Play with clocks (like 5+3; `time off` to stop, `time` to show them)
  hint                 The engine's suggestion for the side to move
  resign, draw         Resign, or offer a draw (the engine accepts if it isn't better)
  pgn                  Show the game as PGN, with the engine's score/depth as comments
  pgn save|load <file> Save the game to a PGN file, or load the first game from one
  go [depth N|nodes N|movetime MS]
                       The engine makes a move now and explains its choice
  play white|black|both|none
                       Which sides the engine plays (it replies to your moves)
  legal, moves         List the legal moves
  perft <depth>        Count the positions <depth> moves ahead, with speed
  divide <depth>       The same, split by first move
  attacks <square>     The squares the piece on <square> attacks
  attackers <square>   The pieces (both colours) attacking <square>
  magic <square>       Step through the magic-bitboard lookup for a rook and bishop there
  zobrist              The position's hash key and the parts it is built from
  set sliders rays|magics
                       Compute sliding attacks by walking rays or by magic lookup
  set depth <N>        The engine searches its moves to depth N
  set movetime <MS>    The engine thinks MS milliseconds per move (default 1000)
  think on|off         Show the engine's thinking, depth by depth (default on)
  set alphabeta on|off Alpha-beta pruning (off: plain minimax, same moves, more nodes)
  set quiescence on|off
                       Search captures past the horizon (off: evaluate mid-exchange)
  set mvvlva on|off    Search captures first, biggest victim first
  set tt on|off        The transposition table: reuse results for positions seen before
  set ttbuckets on|off Look in a bucket of 8 slots (one cache line), not one slot (default off)
  set hash <MB>        Transposition table size in megabytes (default 16)
  set threads <N>      Search threads (Lazy SMP, default 1)
  new960 [n]           Chess960 starting position n (0-959, 518 is the standard one;
                       random without n)
  level <0-20>         The engine's strength level (20 = full strength)
  elo <n>|off          Limit the engine's strength to about n Elo (1000-2400), or not
  set multipv <N>      Search and show the N best lines
  set syzygy <path>    Load Syzygy endgame tablebases from a directory (none: unload)
  tb                   The tablebase result and best move for this position
  set book <path>      Load a Polyglot opening book (.bin; none: unload)
  set ownbook on|off   Play book moves when the book has any (default off)
  book                 The book's moves for this position, with their weights
  set killers|countermoves|history|conthist on|off
                       Quiet-move ordering heuristics (conthist: continuation history)
  set pvs on|off       Principal variation search (null windows after the first move)
  set aspiration on|off
                       Start each depth with a narrow window around the last score
  set nullmove on|off  Null-move pruning
  set lmr on|off       Late move reductions: search late quiet moves less deeply
  set histlmr on|off   Reduce quiet moves with a good history less, a bad one more
  set lmp on|off       Late move pruning: skip late quiet moves near the leaves
  set rfp|razoring|futility|seeprune|delta on|off
                       Reverse futility pruning, razoring, futility pruning, SEE pruning,
                       delta pruning
  set seeorder on|off  Search captures that lose material (by SEE) after quiet moves
  set checkext on|off  Check extensions: search checking moves one ply deeper (default off)
  set singular on|off  Singular extensions (a hash move far better than the rest: a ply deeper)
  set multicut on|off  Multi-cut: cut off a node where two moves beat beta (needs singular)
  set doubleext on|off Double extensions: a hash move singular by a wide margin, two plies
                       deeper (needs singular; default off)
  set negext on|off    Negative extensions: a hash move that isn't singular but beats beta, a
                       ply shallower (needs singular)
  set iir on|off       Internal iterative reductions (no hash move: a ply shallower)
  set improving on|off Adjust pruning when the side to move is improving (default off)
  set tmstability on|off
                       Time: stop sooner when the best move stays the same, later when it changes
  set tmscore on|off   Time: think longer when the score falls from one depth to the next
  set pst on|off       Piece-square tables (off: material only)
  set eval hand|tuned  Hand-set evaluation values (default) or stage 16's tuned ones
  set tapered on|off   Blend middlegame and endgame values by phase (off: middlegame only)
  set mobility|pawns|pieces|kingsafety|tempo on|off
                       Evaluation term groups: mobility, pawn structure and passed pawns,
                       bishop pair and rook files, king safety, tempo
  eval                 The static evaluation of the position, term by term
  eval diff <move>     Which evaluation terms a move changes
  see <move>           Static exchange evaluation: what the captures on its square win
  explain <move>       Search a move and compare it with the engine's choice
  stats                What the last search did: hash hits, cutoffs, each pruning technique
  tree <depth> [plies] The search tree's first plies (default 2): windows, scores, cutoffs,
                       and the moves pruned or reduced
  techniques           Every technique and evaluation term, on or off
  ab <name> [depth]    Search with and without a technique and compare (default depth 8)
  bench                The fixed benchmark: total nodes and speed
  bb <set>             Show a bitboard: occupied, empty, white, black, checkers,
                       pinned, or a piece letter (P N B R Q K for White, p n b r q k)
  match <engine> <games> [tc] [chess960] [openings <file>] [pgn <file>]
                       A short match against another UCI engine (default 10+0.1 per
                       game; also nodes=N or depth=N), saved as PGN, with an Elo estimate
  selfplay <games> [tc] ...
                       The same against a copy of cb2026 at default settings
  uci, xboard          Switch this session to the UCI or XBoard protocol
  help                 This list
  quit, exit           Leave the shell

Every search and evaluation technique can be switched off with `set`, to see what it's worth.";

/// The result of running one command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// Text to show the user (may be empty).
    pub output: String,
    /// Whether the shell should stop after this command.
    pub quit: bool,
    /// A protocol to switch the session to (`uci` or `xboard`): the shell stops, and the
    /// program carries on in that protocol with the rest of the input.
    pub switch_to: Option<Protocol>,
}

impl Response {
    fn text(output: impl Into<String>) -> Response {
        Response {
            output: output.into(),
            ..Response::default()
        }
    }
}

/// The shell's state: the game being played, and how to draw it.
#[derive(Clone, Debug)]
pub struct Shell {
    /// The game: the current position, the moves that led to it, and whether it is over.
    game: Game,
    /// Moves taken back with `undo`, most recent last, for `redo`.
    future: Vec<Move>,
    /// The chess clocks, if the game has them.
    clock: Option<Clock>,
    style: Style,
    engine: Engine,
    /// Which sides the engine plays, indexed by [`Color::index`].
    engine_plays: [bool; 2],
    /// How long the engine thinks about its own moves.
    engine_limit: EngineLimit,
    /// Whether to show the engine's thinking, depth by depth.
    think: bool,
    /// What the last search did, for `stats`.
    last_stats: Option<SearchStats>,
    /// Whether a person is typing at a terminal (long commands then redraw a live board).
    interactive: bool,
}

impl Shell {
    /// A shell at the starting position, drawing boards in `style`.
    pub fn new(style: Style) -> Shell {
        Shell::with_engine(style, Engine::new())
    }

    /// A shell using `engine` (tests pass a seeded engine for repeatable games).
    pub fn with_engine(style: Style, engine: Engine) -> Shell {
        Shell {
            game: Game::new(),
            future: Vec::new(),
            clock: None,
            style,
            engine,
            engine_plays: [false, false],
            engine_limit: EngineLimit::Movetime(DEFAULT_MOVETIME_MS),
            think: true,
            last_stats: None,
            interactive: false,
        }
    }

    /// The current position.
    pub fn position(&self) -> &Position {
        self.game.position()
    }

    /// The engine, with the settings made in the shell.
    pub fn into_engine(self) -> Engine {
        self.engine
    }

    /// Runs one command line.
    pub fn execute(&mut self, line: &str) -> Response {
        self.execute_with(line, &mut |_| {})
    }

    /// Runs one command line, passing progress reports from long commands (`match`) to
    /// `progress` as they happen.
    pub fn execute_with(&mut self, line: &str, progress: &mut dyn FnMut(&str)) -> Response {
        let line = line.trim();
        let (command, args) = match line.split_once(char::is_whitespace) {
            Some((command, args)) => (command, args.trim()),
            None => (line, ""),
        };
        match command {
            "" => Response::default(),
            "help" | "?" => Response::text(HELP),
            "quit" | "exit" => Response {
                quit: true,
                ..Response::default()
            },
            "uci" | "xboard" => Response {
                switch_to: Some(if command == "uci" {
                    Protocol::Uci
                } else {
                    Protocol::Xboard
                }),
                ..Response::default()
            },
            "match" => self.play_match(args, false, progress),
            "selfplay" => self.play_match(args, true, progress),
            "new" | "startpos" => {
                self.set_position(Position::startpos());
                self.show()
            }
            "new960" => {
                let number = if args.is_empty() {
                    // Any number will do: the clock's nanoseconds are random enough here.
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(518, |d| d.subsec_nanos() % 960)
                } else {
                    match args.parse::<u32>() {
                        Ok(n) if n < 960 => n,
                        _ => return Response::text("Usage: new960 [0-959]"),
                    }
                };
                let position = Position::chess960(number).expect("number checked");
                self.set_position(position);
                let mut response = self.show();
                response.output = format!("Chess960 position {number}\n{}", response.output);
                response
            }
            "fen" if args.is_empty() => Response::text(self.game.position().to_fen()),
            "fen" => match Position::from_fen(args) {
                Ok(position) => {
                    self.set_position(position);
                    self.show()
                }
                Err(e) => Response::text(format!("Invalid FEN: {e}")),
            },
            "show" | "d" => self.show(),
            "flip" => {
                self.style.flipped = !self.style.flipped;
                self.show()
            }
            "board" => match args {
                "unicode" => {
                    self.style.unicode = true;
                    self.show()
                }
                "ascii" => {
                    self.style.unicode = false;
                    self.show()
                }
                _ => Response::text("Usage: board unicode|ascii"),
            },
            "color" | "colour" => match args {
                "on" => {
                    self.style.color = true;
                    self.show()
                }
                "off" => {
                    self.style.color = false;
                    self.show()
                }
                _ => Response::text("Usage: color on|off"),
            },
            "bb" => match self.named_bitboard(args) {
                Some(bb) => Response::text(render::bitboard(bb, self.style)),
                None => Response::text(
                    "Usage: bb occupied|empty|white|black|checkers|pinned|<piece letter, e.g. P or n>",
                ),
            },
            "move" | "m" => self.play(args),
            "undo" => match self.game.undo() {
                Some(played) => {
                    self.future.push(played.mv);
                    self.show()
                }
                None => Response::text("Nothing to undo."),
            },
            "redo" => self.redo(),
            "time" => self.time_control(args),
            "hint" => self.hint(),
            "resign" => self.resign(),
            "draw" => self.draw(),
            "pgn" => self.pgn(args),
            "legal" | "moves" => self.legal(),
            "perft" | "divide" => match args.parse::<u32>() {
                Ok(depth) if (1..=10).contains(&depth) => self.perft(depth, command == "divide"),
                _ => Response::text(format!("Usage: {command} <depth 1-10>")),
            },
            "attacks" | "attackers" => match args.parse::<Square>() {
                Ok(square) => self.attacks(square, command == "attackers"),
                Err(e) => Response::text(format!("Usage: {command} <square>: {e}")),
            },
            "magic" => match args.parse::<Square>() {
                Ok(square) => self.magic(square),
                Err(e) => Response::text(format!("Usage: magic <square>: {e}")),
            },
            "set" => self.set(args),
            "go" => self.go(args),
            "bench" => {
                let (nodes, time) = bench::bench(&mut self.engine.clone(), bench::BENCH_DEPTH);
                Response::text(format!(
                    "Bench: {nodes} nodes in {:.2} s ({:.0} nodes/s)",
                    time.as_secs_f64(),
                    nodes as f64 / time.as_secs_f64().max(1e-9)
                ))
            }
            "play" => self.set_engine_sides(args),
            "think" => match args {
                "on" | "off" => {
                    self.think = args == "on";
                    Response::text(format!("Show thinking: {args}"))
                }
                _ => Response::text("Usage: think on|off"),
            },
            "zobrist" => self.zobrist(),
            "eval" if args.is_empty() => self.eval(),
            "eval" => match args.strip_prefix("diff") {
                Some(mv) => self.eval_diff(mv.trim()),
                None => Response::text("Usage: eval | eval diff <move>"),
            },
            "see" => self.see_move(args),
            "explain" => self.explain(args),
            "stats" => self.stats(),
            "tree" => self.tree(args),
            "techniques" => self.techniques(),
            "ab" => self.ab(args),
            "tb" => self.tablebase(),
            "level" => match args.parse::<u32>() {
                Ok(level) if level <= levels::FULL_STRENGTH => {
                    self.engine.skill_level = level;
                    Response::text(self.strength_text())
                }
                _ => Response::text("Usage: level <0-20>"),
            },
            "elo" => match args {
                "off" => {
                    self.engine.limit_strength = false;
                    Response::text(self.strength_text())
                }
                _ => match args.parse::<u32>() {
                    Ok(elo) if (levels::ELO_RANGE.0..=levels::ELO_RANGE.1).contains(&elo) => {
                        self.engine.limit_strength = true;
                        self.engine.elo = elo;
                        Response::text(self.strength_text())
                    }
                    _ => Response::text("Usage: elo <1000-2400>|off"),
                },
            },
            "book" => self.book_moves(),
            // Anything else that is a legal move is played.
            other if parse_move(self.game.position(), other).is_some() => self.play(line),
            other => Response::text(format!(
                "Unknown command '{other}'. Type 'help' for a list."
            )),
        }
    }

    fn show(&self) -> Response {
        let last = self.game.last_move().map(|played| {
            let number = played.before.fullmove_number();
            match played.before.side_to_move() {
                Color::White => format!("{number}. {}", played.san),
                Color::Black => format!("{number}... {}", played.san),
            }
        });
        let mut board = render::board(self.game.position(), self.style, last.as_deref());
        if let Some(clocks) = self.clock_line() {
            board.push_str(&clocks);
            board.push('\n');
        }
        Response::text(board)
    }

    /// Starts over from `position`, forgetting the move history.
    fn set_position(&mut self, position: Position) {
        self.set_game(Game::from_position(position));
    }

    /// Carries on with `game` (a new one, or one loaded from PGN), with fresh clocks.
    fn set_game(&mut self, game: Game) {
        self.game = game;
        self.future.clear();
        if let Some(clock) = self.clock.as_mut() {
            clock.restart();
        }
    }

    /// Plays the space-separated moves in `moves`, stopping at the first one that isn't legal,
    /// then lets the engine reply if it plays the side to move.
    fn play(&mut self, moves: &str) -> Response {
        if moves.is_empty() {
            return Response::text("Usage: move <move> [<move> ...], e.g. move e2e4 e7e5");
        }
        for text in moves.split_whitespace() {
            if let Some(result) = self.game_over() {
                return Response::text(format!("The game is over: {result} Type 'new' or 'undo'."));
            }
            let Some(mv) = parse_move(self.game.position(), text) else {
                let mut response = self.show();
                response.output.push_str(&format!(
                    "'{text}' is not a legal move here. Type 'legal' to list them.\n"
                ));
                return response;
            };
            self.make_move(mv, None);
        }
        self.engine_replies(String::new())
    }

    /// Plays `mv` (with a comment for PGN), stopping the mover's clock. Callers pass a legal
    /// move in a game that isn't over.
    fn make_move(&mut self, mv: Move, comment: Option<String>) {
        self.run_clock();
        if self.game.outcome().is_some() {
            // The mover's flag fell: the move came too late to count.
            return;
        }
        // Replaying the move `undo` took back keeps the rest of the `redo` list.
        if self.future.last() == Some(&mv) {
            self.future.pop();
        } else {
            self.future.clear();
        }
        self.game
            .play_with_comment(mv, comment)
            .expect("the shell plays only legal moves, and only while the game goes on");
    }

    /// The engine's choice in the current position, with its thinking (one line per depth,
    /// if `think` is on) and a summary.
    fn engine_move(&mut self, limits: &Limits) -> Option<(Move, String, Option<String>)> {
        let stop = AtomicBool::new(false);
        let mut infos: Vec<SearchInfo> = Vec::new();
        let result = self.engine.search(
            self.game.position(),
            &self.game.history_hashes(),
            limits,
            &stop,
            &mut |info: &SearchInfo| infos.push(info.clone()),
        );
        self.last_stats = Some(result.stats);
        let mv = result.best_move?;
        let mut text = String::new();
        if self.think {
            for info in &infos {
                let line = if self.engine.multipv > 1 {
                    format!(" #{}", info.multipv)
                } else {
                    String::new()
                };
                text.push_str(&format!(
                    "  depth {:>2}{line}  {:>7}  {:>10} nodes  {:>6} ms  {}\n",
                    info.depth,
                    score_text(info.score),
                    info.nodes,
                    info.time.as_millis(),
                    pv_san(self.game.position(), &info.pv)
                ));
            }
        }
        let san = self.game.position().to_san(mv);
        // The summary describes the main line (a weaker level may still play another move).
        let main = infos.iter().rev().find(|info| info.multipv == 1);
        let comment = main.map(|info| format!("{}/{}", score_text(info.score), info.depth));
        match main {
            Some(info) => text.push_str(&format!(
                "{NAME} plays {san}   (depth {}, {}, {} nodes, {} ms)",
                info.depth,
                score_text(info.score),
                info.nodes,
                info.time.as_millis(),
            )),
            None => text.push_str(&format!("{NAME} plays {san}")),
        }
        Some((mv, text, comment))
    }

    /// The limits for the engine's own moves: the clock if the game has one, else
    /// `set depth` or `set movetime`.
    fn default_limits(&self) -> Limits {
        if let Some(limits) = self.clock_limits() {
            return limits;
        }
        match self.engine_limit {
            EngineLimit::Depth(depth) => Limits::depth(depth),
            EngineLimit::Movetime(ms) => Limits {
                movetime: Some(Duration::from_millis(ms)),
                ..Limits::default()
            },
        }
    }

    /// Lets the engine move while it is the engine's turn and the game isn't over, then shows
    /// the board (after `log`, the moves already reported).
    fn engine_replies(&mut self, mut log: String) -> Response {
        while self.game_over().is_none()
            && self.engine_plays[self.game.position().side_to_move().index()]
        {
            let limits = self.default_limits();
            let Some((mv, summary, comment)) = self.engine_move(&limits) else {
                break;
            };
            log.push_str(&summary);
            log.push('\n');
            self.make_move(mv, comment);
        }
        let mut response = self.show();
        response.output.insert_str(0, &log);
        if let Some(result) = self.game_over() {
            response.output.push_str(&result);
            response.output.push('\n');
        }
        response
    }

    /// `go [depth N] [nodes N] [movetime MS]`: the engine makes one move now, whoever's turn
    /// it is.
    fn go(&mut self, args: &str) -> Response {
        if let Some(result) = self.game_over() {
            return Response::text(format!("The game is over: {result}"));
        }
        let mut limits = self.default_limits();
        let words: Vec<&str> = args.split_whitespace().collect();
        for pair in words.chunks(2) {
            let value = pair.get(1).and_then(|v| v.parse::<u64>().ok());
            match (pair[0], value) {
                ("depth", Some(d)) => limits.depth = Some(d as u32),
                ("nodes", Some(n)) => {
                    limits.nodes = Some(n);
                    limits.depth = Some(search::MAX_PLY);
                }
                ("movetime", Some(ms)) => {
                    limits.movetime = Some(Duration::from_millis(ms));
                    limits.depth = Some(search::MAX_PLY);
                }
                _ => return Response::text("Usage: go [depth N] [nodes N] [movetime MS]"),
            }
        }
        let Some((mv, summary, comment)) = self.engine_move(&limits) else {
            return Response::text("No legal moves.");
        };
        self.make_move(mv, comment);
        self.engine_replies(format!("{summary}\n"))
    }

    /// `play white|black|both|none`: which sides the engine plays from now on.
    fn set_engine_sides(&mut self, args: &str) -> Response {
        self.engine_plays = match args {
            "white" => [true, false],
            "black" => [false, true],
            "both" => [true, true],
            "none" => [false, false],
            _ => return Response::text("Usage: play white|black|both|none"),
        };
        self.engine_replies(String::new())
    }

    /// Why the game is over in the current position, if it is.
    fn game_over(&self) -> Option<String> {
        self.game.outcome().map(|outcome| outcome.to_string())
    }

    fn legal(&self) -> Response {
        let mut moves: Vec<String> = self
            .game
            .position()
            .legal_moves()
            .iter()
            .map(|&mv| self.game.position().to_san(mv))
            .collect();
        moves.sort();
        let mut out = format!("{} legal move{}", moves.len(), plural(moves.len()));
        if !moves.is_empty() {
            out.push_str(":\n");
            for row in moves.chunks(10) {
                out.push_str("  ");
                out.push_str(&row.join(" "));
                out.push('\n');
            }
        }
        Response::text(out)
    }

    fn perft(&self, depth: u32, split: bool) -> Response {
        let start = Instant::now();
        let mut out = String::new();
        let nodes = if split {
            let mut lines: Vec<(String, u64)> = divide(self.game.position(), depth)
                .into_iter()
                .map(|(mv, n)| (mv.to_string(), n))
                .collect();
            lines.sort();
            for (mv, n) in &lines {
                out.push_str(&format!("  {mv}: {n}\n"));
            }
            lines.iter().map(|(_, n)| n).sum()
        } else {
            perft(self.game.position(), depth)
        };
        let seconds = start.elapsed().as_secs_f64();
        out.push_str(&format!(
            "Depth {depth}: {nodes} nodes in {seconds:.3} s ({:.1} million nodes/s)\n",
            nodes as f64 / seconds.max(1e-9) / 1e6
        ));
        Response::text(out)
    }

    /// Shows each step of the magic lookup for a rook and a bishop on `square`, using the
    /// current position's occupied squares.
    fn magic(&self, square: Square) -> Response {
        let occupied = self.game.position().occupied();
        let mut out = String::new();
        let sliders: [(&str, Magic, Bitboard); 2] = [
            (
                "Rook",
                magic::rook_magic(square),
                magic::rook_attacks(square, occupied),
            ),
            (
                "Bishop",
                magic::bishop_magic(square),
                magic::bishop_attacks(square, occupied),
            ),
        ];
        for (name, m, attacks) in sliders {
            let relevant = occupied & m.mask;
            out.push_str(&format!(
                "{name} on {square}\n\
                 1. Relevant squares (mask), {} of them:\n{}\
                 2. Occupied relevant squares: {:#018x}\n\
                 3. Multiply by the magic {:#018x} and keep the top {} bits: index {}\n   \
                    (table offset {} + {})\n\
                 4. The table entry is the attack set:\n{}\n",
                m.mask.count(),
                render::bitboard(m.mask, self.style),
                relevant.0,
                m.magic,
                64 - m.shift,
                m.index(occupied),
                m.offset,
                m.index(occupied) - m.offset,
                render::bitboard(attacks, self.style),
            ));
        }
        Response::text(out)
    }

    /// Lists every part of the position's Zobrist key, then their XOR.
    /// The engine's strength setting, in words.
    fn strength_text(&self) -> String {
        let level = self.engine.level();
        if level >= levels::FULL_STRENGTH {
            return "Strength: full".to_string();
        }
        let settings = levels::settings(level);
        let elo = if self.engine.limit_strength {
            format!(", about {} Elo (provisional)", self.engine.elo)
        } else {
            String::new()
        };
        format!(
            "Strength: level {level}{elo}: {} nodes per move, choosing among the best {} moves",
            settings.nodes.unwrap_or_default(),
            settings.candidates
        )
    }

    /// `book`: the opening book's moves for the position.
    fn book_moves(&self) -> Response {
        let Some(book) = &self.engine.book else {
            return Response::text("No opening book loaded (set book <path>).");
        };
        let moves = book.moves(self.game.position());
        if moves.is_empty() {
            return Response::text("The book has no moves for this position.");
        }
        let total: u32 = moves.iter().map(|m| u32::from(m.weight)).sum();
        let mut out = String::from("Book moves:\n");
        for m in moves {
            let share = u32::from(m.weight) * 100 / total.max(1);
            out.push_str(&format!(
                "  {:<8} weight {:>5}  ({share}%)\n",
                self.game.position().to_san(m.mv),
                m.weight
            ));
        }
        out.push_str(if self.engine.own_book {
            "The engine plays from the book (ownbook on)."
        } else {
            "The engine ignores the book (set ownbook on to use it)."
        });
        Response::text(out)
    }

    /// `tb`: what the endgame tablebases say about the position.
    fn tablebase(&self) -> Response {
        let Some(tb) = &self.engine.tablebase else {
            return Response::text("No tablebases loaded (set syzygy <path>).");
        };
        let pos = self.game.position();
        if !tablebase::covers(tb.as_ref(), &self.engine.tablebase_params, pos) {
            return Response::text(format!(
                "Not in the tablebases: they cover up to {} pieces without castling rights.",
                tb.max_pieces()
                    .min(self.engine.tablebase_params.probe_limit)
            ));
        }
        let side = pos.side_to_move();
        Response::text(match tb.probe_root(pos) {
            Some((wdl, mv)) => {
                let verdict = match wdl {
                    TbWdl::Win => format!("{side} to move wins"),
                    TbWdl::CursedWin => format!(
                        "{side} to move can force mate, but not before the fifty-move rule: a draw"
                    ),
                    TbWdl::Draw => "a draw".to_string(),
                    TbWdl::BlessedLoss => format!(
                        "{side} to move is lost, but the fifty-move rule saves the game: a draw"
                    ),
                    TbWdl::Loss => format!("{side} to move loses"),
                };
                format!("Tablebase: {verdict}. Best move: {}", pos.to_san(mv))
            }
            None => "The tablebases have no answer for this position.".to_string(),
        })
    }

    /// `eval`: the static evaluation, term by term, for both sides.
    fn eval(&self) -> Response {
        let params = &self.engine.params;
        let t = eval::trace(self.game.position(), params);
        let blend = |(mg, eg): (i32, i32)| {
            if params.tapered {
                (mg * t.phase + eg * (eval::MAX_PHASE - t.phase)) / eval::MAX_PHASE
            } else {
                mg
            }
        };
        let mut out = String::from("Evaluation in pawns, each side from its own point of view:\n");
        out.push_str(&format!(
            "  {:<16}{:>16}{:>16}{:>9}\n",
            "", "White", "Black", "Net"
        ));
        out.push_str(&format!(
            "  {:<16}{:>8}{:>8}{:>8}{:>8}{:>9}\n",
            "", "Middle", "End", "Middle", "End", "(W-B)"
        ));
        for (name, [white, black]) in t.terms() {
            let net = blend((white.0 - black.0, white.1 - black.1));
            out.push_str(&format!(
                "  {name:<16}{:>8}{:>8}{:>8}{:>8}{:>9}\n",
                pawns(white.0),
                pawns(white.1),
                pawns(black.0),
                pawns(black.1),
                pawns(net),
            ));
        }
        out.push_str(&format!(
            "Phase {}/{}: {}% middlegame, {}% endgame{}\n",
            t.phase,
            eval::MAX_PHASE,
            t.phase * 100 / eval::MAX_PHASE,
            100 - t.phase * 100 / eval::MAX_PHASE,
            if params.tapered {
                ""
            } else {
                " (tapering off: middlegame values only)"
            },
        ));
        let side = self.game.position().side_to_move();
        out.push_str(&format!(
            "Total: {} for White, so {} for {side} to move",
            pawns(t.total),
            pawns(eval::evaluate(self.game.position(), params)),
        ));
        Response::text(out)
    }

    fn zobrist(&self) -> Response {
        let pos = self.game.position();
        let mut out = String::from("The key is the XOR of one random number per feature:\n");
        let mut total = 0u64;
        let mut add = |label: String, key: u64| {
            out.push_str(&format!("  {key:#018x}  {label}\n"));
            total ^= key;
        };
        for square in Square::all() {
            if let Some(piece) = pos.piece_at(square) {
                add(
                    format!("{piece} on {square}"),
                    zobrist::piece_square(piece, square),
                );
            }
        }
        if pos.side_to_move() == Color::Black {
            add("Black to move".to_string(), zobrist::black_to_move());
        }
        if !pos.castling().is_empty() {
            add(
                "castling rights".to_string(),
                zobrist::castling(pos.castling()),
            );
        }
        if let Some(square) = pos.en_passant() {
            add(
                format!("en passant on the {}-file", square.file().to_char()),
                zobrist::en_passant(square.file()),
            );
        }
        out.push_str(&format!(
            "  ------------------\n  {total:#018x}  = the key (the position says {:#018x})\n",
            pos.hash()
        ));
        Response::text(out)
    }

    /// `set <option> <value>`: engine settings that are useful to switch while learning.
    fn set(&mut self, args: &str) -> Response {
        let mut words = args.split_whitespace();
        let on_off_text = |on: bool| if on { "on" } else { "off" };
        let on_off = |word: Option<&str>| match word {
            Some("on") => Some(true),
            Some("off") => Some(false),
            _ => None,
        };
        match (words.next(), words.next()) {
            (Some("sliders"), Some("rays")) => set_slider_method(SliderMethod::Rays),
            (Some("sliders"), Some("magics")) => set_slider_method(SliderMethod::Magics),
            (Some("depth"), Some(d)) => match d.parse::<u32>() {
                Ok(depth) if (1..=search::MAX_PLY).contains(&depth) => {
                    self.engine_limit = EngineLimit::Depth(depth)
                }
                _ => return Response::text("Usage: set depth <1-128>"),
            },
            (Some("movetime"), Some(ms)) => match ms.parse::<u64>() {
                Ok(ms) if ms > 0 => self.engine_limit = EngineLimit::Movetime(ms),
                _ => return Response::text("Usage: set movetime <milliseconds>"),
            },
            (Some("eval"), Some(which @ ("hand" | "tuned"))) => {
                let values = if which == "tuned" {
                    eval::EvalParams::tuned()
                } else {
                    eval::EvalParams::hand_set()
                };
                self.engine.params.copy_values(&values);
                return Response::text(format!(
                    "Evaluation values: {}",
                    if which == "tuned" {
                        "tuned (stage 16's Texel tuning)"
                    } else {
                        "hand-set (the default)"
                    }
                ));
            }
            (Some(name), value)
                if on_off(value).is_some()
                    && let Some(s) = explain::switch(name) =>
            {
                *(s.flag)(&mut self.engine) = on_off(value).unwrap_or(true);
            }
            (Some("book"), path) => {
                let path = path.unwrap_or("");
                if path.is_empty() {
                    self.engine.book = None;
                    return Response::text("No opening book.");
                }
                return Response::text(
                    match std::fs::read(path)
                        .map_err(|e| format!("{path}: {e}"))
                        .and_then(|bytes| OpeningBook::from_bytes(&bytes))
                    {
                        Ok(book) => {
                            let message = format!("Opening book with {} entries.", book.len());
                            self.engine.book = Some(std::sync::Arc::new(book));
                            message
                        }
                        Err(e) => format!("Opening book not loaded: {e}"),
                    },
                );
            }
            (Some("ownbook"), value) if on_off(value).is_some() => {
                self.engine.own_book = on_off(value).unwrap_or(false);
            }
            (Some("syzygy"), path) => {
                let path = path.unwrap_or("");
                return Response::text(match self.engine.load_tablebases(path) {
                    Ok(summary) => summary,
                    Err(e) => format!("Tablebases not loaded: {e}"),
                });
            }
            (Some("multipv"), Some(n)) => match n.parse::<usize>() {
                Ok(n) if (1..=256).contains(&n) => self.engine.multipv = n,
                _ => return Response::text("Usage: set multipv <1-256>"),
            },
            (Some("threads"), Some(n)) => match n.parse::<usize>() {
                Ok(n) if (1..=MAX_THREADS).contains(&n) => self.engine.threads = n,
                _ => return Response::text(format!("Usage: set threads <1-{MAX_THREADS}>")),
            },
            (Some("hash"), Some(mb)) => match mb.parse::<usize>() {
                Ok(mb) if (1..=MAX_HASH_MB).contains(&mb) => self.engine.set_hash_size(mb),
                _ => return Response::text(format!("Usage: set hash <1-{MAX_HASH_MB}>")),
            },
            (None, _) => {}
            _ => return Response::text(SET_USAGE),
        }
        Response::text(format!(
            "Sliding attacks: {}\nEngine moves: {}\nAlpha-beta pruning: {}\nQuiescence search: {}\nMVV-LVA ordering: {}\nTransposition table: {} ({} MB, buckets {})\nThreads: {}\nPiece-square tables: {}\nTapered evaluation: {}\nMobility, pawns, pieces, king safety, tempo: {}, {}, {}, {}, {}\nKillers, countermoves, history, continuation history: {}, {}, {}, {}\nPVS, aspiration windows: {}, {}\nNull-move pruning: {}\nLate move reductions, adjusted by history, pruning: {}, {}, {}\nReverse futility, razoring, futility: {}, {}, {}\nSEE pruning, delta pruning, SEE ordering: {}, {}, {}",
            match slider_method() {
                SliderMethod::Rays => "walking rays",
                SliderMethod::Magics => "magic bitboards",
            },
            match self.engine_limit {
                EngineLimit::Depth(depth) => format!("search to depth {depth}"),
                EngineLimit::Movetime(ms) => format!("think for {ms} ms"),
            },
            if self.engine.techniques.alpha_beta {
                "on"
            } else {
                "off (plain minimax)"
            },
            on_off_text(self.engine.techniques.quiescence),
            on_off_text(self.engine.techniques.mvv_lva),
            on_off_text(self.engine.techniques.tt),
            self.engine.tt.megabytes(),
            on_off_text(self.engine.techniques.tt_buckets),
            self.engine.threads,
            on_off_text(self.engine.params.use_pst),
            on_off_text(self.engine.params.tapered),
            on_off_text(self.engine.params.terms.use_mobility),
            on_off_text(self.engine.params.terms.use_pawns),
            on_off_text(self.engine.params.terms.use_pieces),
            on_off_text(self.engine.params.terms.use_king_safety),
            on_off_text(self.engine.params.terms.use_tempo),
            on_off_text(self.engine.techniques.killers),
            on_off_text(self.engine.techniques.countermoves),
            on_off_text(self.engine.techniques.history),
            on_off_text(self.engine.techniques.continuation_history),
            on_off_text(self.engine.techniques.pvs),
            on_off_text(self.engine.techniques.aspiration),
            on_off_text(self.engine.techniques.null_move),
            on_off_text(self.engine.techniques.lmr),
            on_off_text(self.engine.techniques.history_lmr),
            on_off_text(self.engine.techniques.late_move_pruning),
            on_off_text(self.engine.techniques.reverse_futility),
            on_off_text(self.engine.techniques.razoring),
            on_off_text(self.engine.techniques.futility),
            on_off_text(self.engine.techniques.see_pruning),
            on_off_text(self.engine.techniques.delta_pruning),
            on_off_text(self.engine.techniques.see_ordering),
        ))
    }

    fn attacks(&self, square: Square, attackers: bool) -> Response {
        let pos = self.game.position();
        if attackers {
            let bb = pos.attackers_to(square, pos.occupied());
            let mut out = format!("Pieces attacking {square}:\n");
            out.push_str(&render::bitboard(bb, self.style));
            return Response::text(out);
        }
        let Some(piece) = pos.piece_at(square) else {
            return Response::text(format!("{square} is empty."));
        };
        let occupied = pos.occupied();
        let bb = match piece.piece_type {
            PieceType::Pawn => pawn_attacks(piece.color, square),
            PieceType::Knight => knight_attacks(square),
            PieceType::Bishop => bishop_attacks(square, occupied),
            PieceType::Rook => rook_attacks(square, occupied),
            PieceType::Queen => queen_attacks(square, occupied),
            PieceType::King => king_attacks(square),
        };
        let mut out = format!("Squares the piece on {square} ({piece}) attacks:\n");
        out.push_str(&render::bitboard(bb, self.style));
        Response::text(out)
    }

    /// The bitboard a `bb` argument names, if any.
    fn named_bitboard(&self, name: &str) -> Option<Bitboard> {
        let pos = self.game.position();
        match name {
            "occupied" | "all" => Some(pos.occupied()),
            "empty" => Some(!pos.occupied()),
            "white" => Some(pos.pieces_of_color(Color::White)),
            "black" => Some(pos.pieces_of_color(Color::Black)),
            "checkers" => Some(pos.checkers()),
            "pinned" => Some(pos.pinned()),
            _ => {
                let mut chars = name.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => {
                        let piece = Piece::from_fen_char(c)?;
                        Some(pos.pieces(piece.color, piece.piece_type))
                    }
                    _ => None,
                }
            }
        }
    }
}

/// Reads a move in UCI notation (`g1f3`) or algebraic notation (`Nf3`).
fn parse_move(pos: &Position, text: &str) -> Option<Move> {
    pos.parse_uci_move(text)
        .or_else(|| pos.parse_san(text).ok())
}

/// Usage for the `set` command.
const SET_USAGE: &str = "Usage: set sliders rays|magics | set depth <N> | set movetime <MS> | set hash <MB> | set threads <N> | set eval hand|tuned | set alphabeta|quiescence|mvvlva|tt|ttbuckets|killers|countermoves|history|conthist|pvs|aspiration|nullmove|lmr|histlmr|lmp|rfp|razoring|futility|seeprune|delta|seeorder|checkext|singular|doubleext|multicut|negext|iir|improving|tmstability|tmscore|pst|tapered|mobility|pawns|pieces|kingsafety|tempo on|off";

/// How long the engine thinks per move by default in the shell.
const DEFAULT_MOVETIME_MS: u64 = 1000;

/// How the shell limits the engine's own moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EngineLimit {
    /// Search to this depth.
    Depth(u32),
    /// Think for this many milliseconds.
    Movetime(u64),
}

/// A score for people: `+0.35` (pawns) or `#3` / `#-2` (mate in moves).
fn score_text(score: Score) -> String {
    match score {
        Score::Centipawns(cp) => format!("{:+.2}", cp as f64 / 100.0),
        Score::Mate(moves) => format!("#{moves}"),
    }
}

/// A centipawn value as pawns, like `+0.35`.
fn pawns(cp: i32) -> String {
    format!("{:+.2}", cp as f64 / 100.0)
}

/// A principal variation in algebraic notation.
fn pv_san(pos: &Position, pv: &[Move]) -> String {
    let mut pos = pos.clone();
    let mut out = Vec::new();
    for &mv in pv {
        if !pos.legal_moves().contains(&mv) {
            break;
        }
        out.push(pos.to_san(mv));
        pos.make_move(mv);
    }
    out.join(" ")
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Runs the shell until `quit`, the end of the input, or `uci`/`xboard`, which returns the
/// protocol to switch to (the caller then carries on with the rest of `input`).
///
/// When `interactive` is true (a person at a terminal) it prints a welcome line, the starting
/// board and a prompt before each command; when reading a script it prints only command
/// output.
pub fn run<R: BufRead, W: Write>(
    shell: &mut Shell,
    input: R,
    mut output: W,
    interactive: bool,
) -> io::Result<Option<Protocol>> {
    shell.interactive = interactive;
    if interactive {
        writeln!(
            output,
            "{} shell. Type 'help' for commands.\n",
            cb2026_engine::id_name()
        )?;
        write!(output, "{}", shell.show().output)?;
    }
    let mut lines = input.lines();
    loop {
        if interactive {
            write!(output, "> ")?;
            output.flush()?;
        }
        let Some(line) = lines.next() else {
            break;
        };
        let line = line?;
        let response = shell.execute_with(&line, &mut |text| {
            // Progress is best effort: a failed write shows up on the next real one.
            let _ = write!(output, "{text}");
            let _ = output.flush();
        });
        if !response.output.is_empty() {
            write!(output, "{}", response.output)?;
            if !response.output.ends_with('\n') {
                writeln!(output)?;
            }
        }
        if response.quit {
            break;
        }
        if response.switch_to.is_some() {
            return Ok(response.switch_to);
        }
    }
    if interactive {
        writeln!(output)?;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

    pub(crate) fn ascii_shell_for_tests() -> Shell {
        ascii_shell()
    }

    fn ascii_shell() -> Shell {
        Shell::with_engine(
            Style {
                unicode: false,
                color: false,
                flipped: false,
            },
            Engine::new(),
        )
    }

    #[test]
    fn undo_and_redo() {
        let mut shell = ascii_shell();
        shell.execute("e4 e5 Nf3");
        shell.execute("undo");
        shell.execute("undo");
        assert_eq!(shell.game.moves().len(), 1);
        shell.execute("redo");
        shell.execute("redo");
        assert_eq!(shell.game.moves().len(), 3);
        assert_eq!(shell.execute("redo").output, "Nothing to redo.");
        // A different move forgets the moves taken back.
        shell.execute("undo");
        shell.execute("Nc3");
        assert_eq!(shell.execute("redo").output, "Nothing to redo.");
    }

    #[test]
    fn resigning_and_draws_end_the_game() {
        let mut shell = ascii_shell();
        shell.execute("e4");
        assert_eq!(
            shell.execute("resign").output,
            "Black resigned. White wins."
        );
        assert!(shell.execute("e5").output.starts_with("The game is over"));
        assert_eq!(shell.game.to_pgn().result, "1-0");
        shell.execute("undo");
        assert_eq!(shell.game.to_pgn().result, "*");
        // Between two people, a draw offer is accepted.
        assert_eq!(shell.execute("draw").output, "Draw agreed.");
        assert_eq!(shell.game.to_pgn().result, "1/2-1/2");
        // The engine, a queen up, declines.
        shell.execute("fen 4k3/8/8/8/8/8/8/3QK3 b - - 0 1");
        shell.execute("set depth 3");
        shell.engine_plays = [true, false];
        let out = shell.execute("draw").output;
        assert!(out.contains("declines"), "{out}");
    }

    #[test]
    fn hints_suggest_without_playing() {
        let mut shell = ascii_shell();
        shell.execute("fen 6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1");
        shell.execute("set depth 3");
        let hint = shell.execute("hint").output;
        assert!(hint.starts_with("Hint: Re8#"), "{hint}");
        assert!(shell.game.moves().is_empty());
    }

    #[test]
    fn pgn_saves_and_loads_games() {
        let mut shell = ascii_shell();
        shell.execute("set depth 2");
        shell.execute("e4");
        shell.execute("go");
        let pgn = shell.execute("pgn").output;
        assert!(pgn.contains("[White \"Player\"]"), "{pgn}");
        // The engine's move carries its score and depth.
        assert!(pgn.contains("1. e4 ") && pgn.contains("/2}"), "{pgn}");
        assert!(pgn.ends_with(" *"), "{pgn}");
        let path = std::env::temp_dir().join(format!("cb2026-shell-{}.pgn", std::process::id()));
        let path = path.to_str().unwrap();
        assert!(
            shell
                .execute(&format!("pgn save {path}"))
                .output
                .starts_with("Saved")
        );
        let fen = shell.position().to_fen();
        shell.execute("new");
        let loaded = shell.execute(&format!("pgn load {path}")).output;
        assert!(loaded.starts_with("Loaded Player vs cb2026"), "{loaded}");
        assert_eq!(shell.position().to_fen(), fen);
        assert_eq!(shell.game.moves().len(), 2);
        std::fs::remove_file(path).unwrap();
        assert!(
            shell
                .execute("pgn load /no/such/file.pgn")
                .output
                .starts_with("Couldn't load")
        );
    }

    #[test]
    fn clocks_run_and_flag() {
        let mut shell = ascii_shell();
        assert!(shell.execute("time 5+3").output.starts_with("Clocks set"));
        let board = shell.execute("e4").output;
        assert!(board.contains("Clocks: White 5:0"), "{board}");
        // A clock too short to make a move in.
        shell.execute("time 0.0001+0");
        std::thread::sleep(Duration::from_millis(50));
        let out = shell.execute("e5").output;
        assert!(out.contains("Black lost on time."), "{out}");
        assert_eq!(shell.game.to_pgn().result, "1-0");
        assert_eq!(shell.execute("time off").output, "Clocks off.");
        assert!(shell.execute("time fast").output.starts_with("Usage"));
    }

    #[test]
    fn techniques_are_listed_and_switched() {
        let mut shell = ascii_shell();
        assert!(
            shell
                .execute("techniques")
                .output
                .contains("nullmove     on")
        );
        shell.execute("set nullmove off");
        assert!(
            shell
                .execute("techniques")
                .output
                .contains("nullmove     off")
        );
        assert!(
            shell
                .execute("set nullmove maybe")
                .output
                .starts_with("Usage")
        );
        let ab = shell.execute("ab lmr 4").output;
        assert!(ab.contains("\n  on ") && ab.contains("\n  off "), "{ab}");
        assert!(ab.contains("× the nodes"), "{ab}");
        assert!(shell.execute("ab frobnicate").output.starts_with("Usage"));
    }

    #[test]
    fn stats_describe_the_last_search() {
        let mut shell = ascii_shell();
        assert!(shell.execute("stats").output.starts_with("No search yet"));
        shell.execute("go depth 4");
        let stats = shell.execute("stats").output;
        assert!(stats.contains("Beta cutoffs:"), "{stats}");
        assert!(stats.contains("by the first move searched"), "{stats}");
    }

    #[test]
    fn explain_compares_a_move_with_the_best() {
        let mut shell = ascii_shell();
        shell.execute("set depth 4");
        let out = shell.execute("explain a3").output;
        assert!(out.contains("Engine's choice"), "{out}");
        assert!(out.contains("Your move a3"), "{out}");
        // The mate in one is the engine's choice too.
        shell.execute("fen 6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1");
        let out = shell.execute("explain Re8+").output;
        assert!(out.contains("is the move the engine would play"), "{out}");
        assert!(shell.execute("explain").output.starts_with("Usage"));
    }

    #[test]
    fn eval_diff_and_see() {
        let mut shell = ascii_shell();
        shell.execute("fen 4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
        let diff = shell.execute("eval diff exd5").output;
        assert!(diff.contains("Material"), "{diff}");
        assert!(diff.contains("Total"), "{diff}");
        let see = shell.execute("see exd5").output;
        assert!(
            see.contains("+1.00") && see.contains("wins material"),
            "{see}"
        );
        assert!(shell.execute("see e2e4").output.starts_with("Usage"));
    }

    #[test]
    fn tree_shows_the_first_plies() {
        let mut shell = ascii_shell();
        let tree = shell.execute("tree 3 1").output;
        assert!(tree.contains("depth 2 ["), "{tree}");
        assert!(tree.contains("Result:"), "{tree}");
        let deeper = shell.execute("tree 4 2").output;
        assert!(deeper.contains("\n  "), "two plies are indented: {deeper}");
        assert!(shell.execute("tree 99").output.starts_with("Usage"));
    }

    #[test]
    fn fen_sets_and_prints_the_position() {
        let mut shell = ascii_shell();
        let shown = shell.execute(&format!("fen {KIWIPETE}"));
        assert!(shown.output.contains("FEN: r3k2r/"));
        assert_eq!(shell.execute("fen").output, KIWIPETE);
        shell.execute("new");
        assert_eq!(shell.position(), &Position::startpos());
    }

    #[test]
    fn bad_fen_keeps_the_old_position() {
        let mut shell = ascii_shell();
        let response = shell.execute("fen not a fen");
        assert!(response.output.starts_with("Invalid FEN: "));
        assert_eq!(shell.position(), &Position::startpos());
    }

    #[test]
    fn bitboards_by_name() {
        let mut shell = ascii_shell();
        assert!(
            shell
                .execute("bb P")
                .output
                .contains("0x000000000000ff00  (8 squares)")
        );
        assert!(
            shell
                .execute("bb k")
                .output
                .contains("0x1000000000000000  (1 square)")
        );
        assert!(shell.execute("bb empty").output.contains("(32 squares)"));
        assert!(
            shell
                .execute("bb white")
                .output
                .contains("0x000000000000ffff")
        );
        assert!(shell.execute("bb x").output.starts_with("Usage: bb"));
    }

    #[test]
    fn style_commands() {
        let mut shell = ascii_shell();
        assert!(shell.execute("board unicode").output.contains('♔'));
        assert!(shell.execute("color on").output.contains("\x1b["));
        assert!(!shell.execute("color off").output.contains("\x1b["));
        assert!(shell.execute("flip").output.starts_with(" 1 "));
        assert!(shell.execute("board sideways").output.starts_with("Usage"));
    }

    #[test]
    fn unknown_commands_and_quit() {
        let mut shell = ascii_shell();
        assert!(
            shell
                .execute("dance")
                .output
                .contains("Unknown command 'dance'")
        );
        assert_eq!(shell.execute("").output, "");
        assert!(shell.execute("quit").quit);
        assert!(shell.execute("exit").quit);
    }

    #[test]
    fn plays_and_undoes_moves() {
        let mut shell = ascii_shell();
        shell.execute("e2e4");
        shell.execute("move e7e5 g1f3");
        assert_eq!(
            shell.position().to_fen(),
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
        );
        shell.execute("undo");
        shell.execute("undo");
        assert_eq!(
            shell.execute("fen").output,
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        );
        let response = shell.execute("move e7e5 e2e4");
        assert!(response.output.contains("'e2e4' is not a legal move here"));
        assert!(shell.execute("undo").output.contains("FEN:"));
        assert!(shell.execute("undo").output.contains("FEN:"));
        assert_eq!(shell.execute("undo").output, "Nothing to undo.");
    }

    #[test]
    fn accepts_algebraic_notation_and_shows_the_last_move() {
        let mut shell = ascii_shell();
        let out = shell.execute("move e4 e5 Nf3 Nc6 Bb5").output;
        assert!(out.contains("Last move: 3. Bb5"), "{out}");
        assert_eq!(
            shell.position().to_fen(),
            "r1bqkbnr/pppp1ppp/2n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3"
        );
        assert!(shell.execute("a6").output.contains("Last move: 3... a6"));
    }

    #[test]
    fn engine_replies_when_it_plays_a_side() {
        let mut shell = ascii_shell();
        assert!(shell.execute("play black").output.contains("White to move"));
        let out = shell.execute("e4").output;
        assert!(out.contains("cb2026 plays "), "{out}");
        assert_eq!(shell.position().side_to_move(), Color::White);
        assert!(shell.execute("go").output.contains("cb2026 plays "));
        assert!(shell.execute("play nobody").output.starts_with("Usage"));
    }

    #[test]
    fn engine_against_itself_plays_to_the_end() {
        let mut shell = ascii_shell();
        // A quick game: a shallow search from a simple endgame.
        shell.execute("set depth 2");
        shell.execute("fen 8/8/4k3/8/8/2K5/3R4/8 w - - 0 1");
        let out = shell.execute("play both").output;
        let finished = [
            "Checkmate",
            "Stalemate",
            "fifty-move",
            "enough material",
            "repetition",
        ];
        assert!(finished.iter().any(|f| out.contains(f)), "{out}");
        assert!(
            shell
                .execute("move e4")
                .output
                .starts_with("The game is over")
        );
    }

    #[test]
    fn go_explains_its_choice() {
        let mut shell = ascii_shell();
        shell.execute("fen 6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1");
        assert!(shell.execute("go sideways").output.starts_with("Usage"));
        let out = shell.execute("go depth 2").output;
        assert!(out.contains("cb2026 plays Re8#"), "{out}");
        assert!(out.contains("#1"), "the score is mate in one: {out}");
        assert!(out.contains("depth  1"), "the thinking is shown: {out}");
        assert!(out.contains("Checkmate. White wins."));
    }

    #[test]
    fn eval_values_can_be_switched() {
        let mut shell = ascii_shell();
        let hand = shell.execute("eval").output;
        assert!(shell.execute("set eval tuned").output.contains("tuned"));
        assert_ne!(shell.execute("eval").output, hand);
        shell.execute("set eval hand");
        assert_eq!(shell.execute("eval").output, hand);
    }

    #[test]
    fn search_settings() {
        let mut shell = ascii_shell();
        assert!(
            shell
                .execute("set depth 3")
                .output
                .contains("search to depth 3")
        );
        assert!(
            shell
                .execute("set movetime 250")
                .output
                .contains("think for 250 ms")
        );
        assert_eq!(shell.execute("think off").output, "Show thinking: off");
        assert!(
            shell
                .execute("set alphabeta off")
                .output
                .contains("plain minimax")
        );
        assert!(
            shell
                .execute("set alphabeta on")
                .output
                .contains("pruning: on")
        );
        assert!(
            shell
                .execute("set quiescence off")
                .output
                .contains("Quiescence search: off")
        );
        assert!(
            shell
                .execute("set mvvlva off")
                .output
                .contains("MVV-LVA ordering: off")
        );
        assert!(
            shell
                .execute("set hash 2")
                .output
                .contains("Transposition table: on (2 MB, buckets off)")
        );
        assert!(
            shell
                .execute("set ttbuckets on")
                .output
                .contains("Transposition table: on (2 MB, buckets on)")
        );
        assert!(
            shell
                .execute("set tt off")
                .output
                .contains("Transposition table: off")
        );
        assert!(shell.execute("set hash 0").output.starts_with("Usage"));
        assert!(shell.execute("set threads 4").output.contains("Threads: 4"));
        assert!(shell.execute("set threads 0").output.starts_with("Usage"));
        assert!(
            shell
                .execute("new960 0")
                .output
                .starts_with("Chess960 position 0")
        );
        assert!(shell.execute("fen").output.starts_with("bbqnnrkr/"));
        assert!(shell.execute("new960 960").output.starts_with("Usage"));
        assert_eq!(shell.execute("level 20").output, "Strength: full");
        assert!(
            shell
                .execute("level 5")
                .output
                .starts_with("Strength: level 5: 3037 nodes")
        );
        assert!(shell.execute("elo 1300").output.contains("about 1300 Elo"));
        assert!(shell.execute("elo 900").output.starts_with("Usage"));
        assert!(shell.execute("level 21").output.starts_with("Usage"));
        shell.execute("elo off");
        shell.execute("level 20");
        shell.execute("set multipv 2");
        shell.execute("new");
        shell.execute("set depth 2");
        shell.execute("think on");
        let out = shell.execute("go").output;
        assert!(out.contains("depth  2 #2"), "{out}");
        shell.execute("set multipv 1");
        shell.execute("new");
        assert_eq!(
            shell.execute("book").output,
            "No opening book loaded (set book <path>)."
        );
        assert!(
            shell
                .execute("set book /no/book.bin")
                .output
                .starts_with("Opening book not loaded")
        );
        assert_eq!(
            shell.execute("tb").output,
            "No tablebases loaded (set syzygy <path>)."
        );
        assert!(
            shell
                .execute("set syzygy /tables")
                .output
                .contains("no tablebase support")
        );
        assert!(shell.execute("set depth 0").output.starts_with("Usage"));
        assert!(shell.execute("set colour blue").output.starts_with("Usage"));
    }

    #[test]
    fn eval_shows_the_terms() {
        let mut shell = ascii_shell();
        let out = shell.execute("eval").output;
        assert!(out.contains("Material"), "{out}");
        assert!(out.contains("King safety"), "{out}");
        assert!(out.contains("Phase 24/24: 100% middlegame"), "{out}");
        // Only the tempo bonus separates the sides at the start.
        let tempo = pawns(shell.engine.params.value(eval::Param::Tempo).0);
        assert!(
            out.ends_with(&format!(
                "Total: {tempo} for White, so {tempo} for White to move"
            )),
            "{out}"
        );
        shell.execute("fen 4k3/8/8/4N3/8/8/8/4K3 b - - 0 1");
        let out = shell.execute("eval").output;
        assert!(out.contains("Phase 1/24"), "{out}");
        for term in [
            "pst",
            "tapered",
            "mobility",
            "pawns",
            "pieces",
            "kingsafety",
            "tempo",
        ] {
            shell.execute(&format!("set {term} off"));
        }
        let out = shell.execute("eval").output;
        let knight = shell
            .engine
            .params
            .value(eval::Param::Material(PieceType::Knight))
            .0;
        assert!(
            out.ends_with(&format!(
                "Total: {} for White, so {} for Black to move",
                pawns(knight),
                pawns(-knight)
            )),
            "{out}"
        );
    }

    #[test]
    fn detects_draws() {
        let mut shell = ascii_shell();
        // Knights out and back twice: the start position for the third time.
        let out = shell.execute("move Nf3 Nf6 Ng1 Ng8 Nf3 Nf6 Ng1 Ng8").output;
        assert!(out.contains("Draw by threefold repetition."), "{out}");
        shell.execute("fen 4k3/8/8/8/8/8/8/4K2N w - - 0 1");
        assert!(shell.execute("Ng3").output.contains("enough material"));
        shell.execute("fen 4k3/8/8/8/8/8/8/R3K3 w - - 99 80");
        assert!(shell.execute("Ra2").output.contains("fifty-move"));
    }

    #[test]
    fn reports_checkmate() {
        let mut shell = ascii_shell();
        let response = shell.execute("move f2f3 e7e5 g2g4 d8h4");
        assert!(response.output.contains("Checkmate. Black wins."));
    }

    #[test]
    fn legal_perft_and_divide() {
        let mut shell = ascii_shell();
        let legal = shell.execute("legal").output;
        assert!(legal.starts_with("20 legal moves:"));
        assert!(
            legal.contains("Nf3"),
            "moves are listed in algebraic notation"
        );
        assert!(
            shell
                .execute("perft 3")
                .output
                .contains("Depth 3: 8902 nodes")
        );
        let split = shell.execute("divide 2").output;
        assert!(split.contains("  e2e4: 20\n"));
        assert!(split.contains("Depth 2: 400 nodes"));
        assert!(shell.execute("perft 0").output.starts_with("Usage"));
    }

    #[test]
    fn attack_views() {
        let mut shell = ascii_shell();
        // e2, f3 and h3: a square holding one of our own pieces still counts as attacked.
        assert!(shell.execute("attacks g1").output.contains("(3 squares)"));
        assert!(shell.execute("attackers f3").output.contains("(3 squares)"));
        assert_eq!(shell.execute("attacks e4").output, "e4 is empty.");
        assert!(shell.execute("attacks z9").output.starts_with("Usage"));
    }

    #[test]
    fn magic_walkthrough() {
        let mut shell = ascii_shell();
        let out = shell.execute("magic d4").output;
        assert!(out.contains("Rook on d4"));
        assert!(out.contains("Relevant squares (mask), 10 of them"));
        assert!(out.contains("Bishop on d4"));
        assert!(shell.execute("magic x").output.starts_with("Usage"));
    }

    #[test]
    fn zobrist_breakdown_adds_up() {
        let mut shell = ascii_shell();
        shell.execute("e2e4");
        let out = shell.execute("zobrist").output;
        assert!(out.contains("Black to move"));
        let hash = format!("{:#018x}", shell.position().hash());
        assert!(out.contains(&format!("{hash}  = the key (the position says {hash})")));
    }

    #[test]
    fn slider_setting() {
        // The setting is global, so this test sets it back to the default at the end.
        let mut shell = ascii_shell();
        assert!(
            shell
                .execute("set sliders rays")
                .output
                .contains("Sliding attacks: walking rays")
        );
        assert!(shell.execute("perft 3").output.contains("8902 nodes"));
        assert!(
            shell
                .execute("set sliders magics")
                .output
                .contains("Sliding attacks: magic bitboards")
        );
    }

    #[test]
    fn runs_a_script() {
        let mut shell = ascii_shell();
        let script = format!("fen {KIWIPETE}\nfen\nquit\nfen\n");
        let mut out = Vec::new();
        run(&mut shell, script.as_bytes(), &mut out, false).unwrap();
        let out = String::from_utf8(out).unwrap();
        // The second `fen` prints the position; nothing runs after `quit`.
        assert_eq!(out.matches(KIWIPETE).count(), 2);
        assert!(!out.contains("> "), "no prompt when not interactive");
    }

    #[test]
    fn interactive_session_shows_prompt_and_board() {
        let mut shell = ascii_shell();
        let mut out = Vec::new();
        run(&mut shell, "quit\n".as_bytes(), &mut out, true).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("shell. Type 'help' for commands."));
        assert!(out.contains("FEN: rnbqkbnr/"));
        assert!(out.contains("> "));
    }
}
