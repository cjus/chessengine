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
//! ## Commands so far (stage 1)
//!
//! | Command | What it does |
//! | --- | --- |
//! | `new`, `startpos` | Set up the starting position |
//! | `fen [FEN]` | Set a position from FEN, or print the current FEN |
//! | `show`, `d` | Draw the board |
//! | `flip` | Draw the board from the other side |
//! | `board unicode\|ascii` | Chess symbols or letters |
//! | `color on\|off` | Coloured squares |
//! | `bb <set>` | Show a bitboard: `occupied`, `empty`, `white`, `black`, or a piece letter (`P`, `n`, ...) |
//! | `help` | List commands |
//! | `quit`, `exit` | Leave |
//!
//! ## Coming in later stages
//!
//! - **Stages 2–3:** moves, `legal`, `perft`, `divide`, attack maps and magic lookups.
//! - **Stage 4:** playing complete games against the engine with clocks and levels, and a live
//!   display of the engine's thinking.
//! - **Stage 9 on:** the explain commands: `eval`, `explain`, `tree` and `stats`.
//! - **M4:** `match`, a short match against another UCI engine.
//!
//! The full plan is in `docs/PRD.md`, "The interactive shell".

#![forbid(unsafe_code)]

pub mod render;

use std::io::{self, BufRead, Write};

use cb2026_chess::{Bitboard, Color, Piece, Position};
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
  bb <set>             Show a bitboard: occupied, empty, white, black,
                       or a piece letter (P N B R Q K for White, p n b r q k for Black)
  help                 This list
  quit, exit           Leave the shell

Moves, perft and playing against the engine arrive in later stages.";

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

/// The shell's state: the current position and how to draw it.
#[derive(Clone, Debug)]
pub struct Shell {
    position: Position,
    style: Style,
}

impl Shell {
    /// A shell at the starting position, drawing boards in `style`.
    pub fn new(style: Style) -> Shell {
        Shell {
            position: Position::startpos(),
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
                self.position = Position::startpos();
                self.show()
            }
            "fen" if args.is_empty() => Response::text(self.position.to_fen()),
            "fen" => match Position::from_fen(args) {
                Ok(position) => {
                    self.position = position;
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
                    "Usage: bb occupied|empty|white|black|<piece letter, e.g. P or n>",
                ),
            },
            other => Response::text(format!(
                "Unknown command '{other}'. Type 'help' for a list."
            )),
        }
    }

    fn show(&self) -> Response {
        Response::text(render::board(&self.position, self.style))
    }

    /// The bitboard a `bb` argument names, if any.
    fn named_bitboard(&self, name: &str) -> Option<Bitboard> {
        let pos = &self.position;
        match name {
            "occupied" | "all" => Some(pos.occupied()),
            "empty" => Some(!pos.occupied()),
            "white" => Some(pos.pieces_of_color(Color::White)),
            "black" => Some(pos.pieces_of_color(Color::Black)),
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
