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
//! ## Commands so far (stages 1–3)
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
//! | `undo` | Take back the last move |
//! | `legal`, `moves` | List the legal moves |
//! | `perft <depth>`, `divide <depth>` | Count positions (move generation check) |
//! | `attacks <square>`, `attackers <square>` | What a piece attacks; what attacks a square |
//! | `magic <square>` | How the magic-bitboard lookup works out a slider's attacks there |
//! | `zobrist` | The position's hash key and the parts it is built from |
//! | `set sliders rays\|magics` | Compute sliding attacks by walking rays or by magic lookup |
//! | `bb <set>` | Show a bitboard: `occupied`, `empty`, `white`, `black`, `checkers`, `pinned`, or a piece letter (`P`, `n`, ...) |
//! | `help` | List commands |
//! | `quit`, `exit` | Leave |
//!
//! ## Coming in later stages
//!
//! - **Stage 4:** moves in algebraic notation (`Nf3`).
//! - **Stage 4:** playing complete games against the engine with clocks and levels, and a live
//!   display of the engine's thinking.
//! - **Stage 9 on:** the explain commands: `eval`, `explain`, `tree` and `stats`.
//! - **M4:** `match`, a short match against another UCI engine.
//!
//! The full plan is in `docs/PRD.md`, "The interactive shell".

#![forbid(unsafe_code)]

pub mod render;

use std::io::{self, BufRead, Write};

use std::time::Instant;

use cb2026_chess::attacks::{
    SliderMethod, bishop_attacks, king_attacks, knight_attacks, pawn_attacks, queen_attacks,
    rook_attacks, set_slider_method, slider_method,
};
use cb2026_chess::magic::{self, Magic};
use cb2026_chess::perft::{divide, perft};
use cb2026_chess::zobrist;
use cb2026_chess::{Bitboard, Color, Move, Piece, PieceType, Position, Square};
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
  undo                 Take back the last move
  legal, moves         List the legal moves
  perft <depth>        Count the positions <depth> moves ahead, with speed
  divide <depth>       The same, split by first move
  attacks <square>     The squares the piece on <square> attacks
  attackers <square>   The pieces (both colours) attacking <square>
  magic <square>       Step through the magic-bitboard lookup for a rook and bishop there
  zobrist              The position's hash key and the parts it is built from
  set sliders rays|magics
                       Compute sliding attacks by walking rays or by magic lookup
  bb <set>             Show a bitboard: occupied, empty, white, black, checkers,
                       pinned, or a piece letter (P N B R Q K for White, p n b r q k)
  help                 This list
  quit, exit           Leave the shell

Playing against the engine arrives in stage 4.";

/// The result of running one command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// Text to show the user (may be empty).
    pub output: String,
    /// Whether the shell should stop after this command.
    pub quit: bool,
}

impl Response {
    fn text(output: impl Into<String>) -> Response {
        Response {
            output: output.into(),
            quit: false,
        }
    }
}

/// The shell's state: the current position, the moves that led to it, and how to draw it.
#[derive(Clone, Debug)]
pub struct Shell {
    position: Position,
    /// Earlier positions and the moves played from them (in SAN), for `undo` and display.
    history: Vec<(Position, String)>,
    style: Style,
}

impl Shell {
    /// A shell at the starting position, drawing boards in `style`.
    pub fn new(style: Style) -> Shell {
        Shell {
            position: Position::startpos(),
            history: Vec::new(),
            style,
        }
    }

    /// The current position.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// Runs one command line.
    pub fn execute(&mut self, line: &str) -> Response {
        let line = line.trim();
        let (command, args) = match line.split_once(char::is_whitespace) {
            Some((command, args)) => (command, args.trim()),
            None => (line, ""),
        };
        match command {
            "" => Response::default(),
            "help" | "?" => Response::text(HELP),
            "quit" | "exit" => Response {
                output: String::new(),
                quit: true,
            },
            "new" | "startpos" => {
                self.set_position(Position::startpos());
                self.show()
            }
            "fen" if args.is_empty() => Response::text(self.position.to_fen()),
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
            "undo" => match self.history.pop() {
                Some((previous, _)) => {
                    self.position = previous;
                    self.show()
                }
                None => Response::text("Nothing to undo."),
            },
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
            "zobrist" => self.zobrist(),
            // Anything else that is a legal move is played.
            other if parse_move(&self.position, other).is_some() => self.play(line),
            other => Response::text(format!(
                "Unknown command '{other}'. Type 'help' for a list."
            )),
        }
    }

    fn show(&self) -> Response {
        let last = self.history.last().map(|(before, san)| {
            let number = before.fullmove_number();
            match before.side_to_move() {
                Color::White => format!("{number}. {san}"),
                Color::Black => format!("{number}... {san}"),
            }
        });
        Response::text(render::board(&self.position, self.style, last.as_deref()))
    }

    /// Starts over from `position`, forgetting the move history.
    fn set_position(&mut self, position: Position) {
        self.position = position;
        self.history.clear();
    }

    /// Plays the space-separated moves in `moves`, stopping at the first one that isn't legal.
    fn play(&mut self, moves: &str) -> Response {
        if moves.is_empty() {
            return Response::text("Usage: move <move> [<move> ...], e.g. move e2e4 e7e5");
        }
        for text in moves.split_whitespace() {
            let Some(mv) = parse_move(&self.position, text) else {
                let mut response = self.show();
                response.output.push_str(&format!(
                    "'{text}' is not a legal move here. Type 'legal' to list them.\n"
                ));
                return response;
            };
            let san = self.position.to_san(mv);
            self.history.push((self.position.clone(), san));
            self.position.make_move(mv);
        }
        let mut response = self.show();
        if let Some(result) = game_over(&self.position) {
            response.output.push_str(&result);
            response.output.push('\n');
        }
        response
    }

    fn legal(&self) -> Response {
        let mut moves: Vec<String> = self
            .position
            .legal_moves()
            .iter()
            .map(|&mv| self.position.to_san(mv))
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
            let mut lines: Vec<(String, u64)> = divide(&self.position, depth)
                .into_iter()
                .map(|(mv, n)| (mv.to_string(), n))
                .collect();
            lines.sort();
            for (mv, n) in &lines {
                out.push_str(&format!("  {mv}: {n}\n"));
            }
            lines.iter().map(|(_, n)| n).sum()
        } else {
            perft(&self.position, depth)
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
        let occupied = self.position.occupied();
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
    fn zobrist(&self) -> Response {
        let pos = &self.position;
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
        match (words.next(), words.next()) {
            (Some("sliders"), Some("rays")) => set_slider_method(SliderMethod::Rays),
            (Some("sliders"), Some("magics")) => set_slider_method(SliderMethod::Magics),
            (Some("sliders"), None) => {}
            _ => return Response::text("Usage: set sliders rays|magics"),
        }
        Response::text(format!(
            "Sliding attacks: {}",
            match slider_method() {
                SliderMethod::Rays => "walking rays",
                SliderMethod::Magics => "magic bitboards",
            }
        ))
    }

    fn attacks(&self, square: Square, attackers: bool) -> Response {
        let pos = &self.position;
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
        let pos = &self.position;
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

/// A message if the game is over in `pos` (checkmate or stalemate).
fn game_over(pos: &Position) -> Option<String> {
    if !pos.legal_moves().is_empty() {
        return None;
    }
    Some(if pos.in_check() {
        format!("Checkmate: {} wins.", !pos.side_to_move())
    } else {
        "Stalemate: the game is a draw.".to_string()
    })
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Runs the shell until `quit` or the end of the input.
///
/// When `interactive` is true (a person at a terminal) it prints a welcome line, the starting
/// board and a prompt before each command; when reading a script it prints only command
/// output.
pub fn run<R: BufRead, W: Write>(
    shell: &mut Shell,
    input: R,
    mut output: W,
    interactive: bool,
) -> io::Result<()> {
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
        let response = shell.execute(&line?);
        if !response.output.is_empty() {
            write!(output, "{}", response.output)?;
            if !response.output.ends_with('\n') {
                writeln!(output)?;
            }
        }
        if response.quit {
            break;
        }
    }
    if interactive {
        writeln!(output)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

    fn ascii_shell() -> Shell {
        Shell::new(Style {
            unicode: false,
            color: false,
            flipped: false,
        })
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
    fn reports_checkmate() {
        let mut shell = ascii_shell();
        let response = shell.execute("move f2f3 e7e5 g2g4 d8h4");
        assert!(response.output.contains("Checkmate: Black wins."));
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
        assert_eq!(
            shell.execute("set sliders rays").output,
            "Sliding attacks: walking rays"
        );
        assert!(shell.execute("perft 3").output.contains("8902 nodes"));
        assert_eq!(
            shell.execute("set sliders magics").output,
            "Sliding attacks: magic bitboards"
        );
        assert!(shell.execute("set colour blue").output.starts_with("Usage"));
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
