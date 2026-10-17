//! Playing one game between two UCI engines, as a GUI or tournament manager would.
//!
//! The referee (this code) keeps the real board and the clocks. Before each move it sends the
//! engine to move the full game (`position fen ... moves ...`) and the clocks (`go wtime ...`),
//! times the reply itself, checks that the move is legal, and ends the game on checkmate,
//! stalemate, the draw rules, a time loss, an illegal move or a crash. The board and the rules
//! that end a game on it are a [`rules::Game`] from `cb2026-chess`.
//!
//! A game that can't start (an engine doesn't answer `ucinewgame`) is **aborted**: it has no
//! result, and a match must not score it. Scoring it as a draw would let a dead engine add
//! draws to a test's evidence (cjus/chessengine#89).

use std::fmt;
use std::time::Duration;

use cb2026_chess::game as rules;
use cb2026_chess::{Color, Position};

use crate::uci_client::UciEngine;

/// How long each side may think.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeControl {
    /// A clock: starting time plus an increment per move, like `8+0.08`.
    Clock {
        /// Starting time on each clock.
        base: Duration,
        /// Time added after each move.
        increment: Duration,
    },
    /// A repeating time control, like `40/60`: `moves` moves in `base`, then the same again
    /// added to the clock, and so on (the engines get `movestogo`).
    Repeating {
        /// Moves per period.
        moves: u32,
        /// Time for each period.
        base: Duration,
    },
    /// A fixed number of nodes per move: no clocks, results don't depend on machine load.
    Nodes(u64),
    /// A fixed depth per move.
    Depth(u32),
}

impl TimeControl {
    /// Parses `8+0.08` (seconds), `40/60` (moves/seconds, repeating), `nodes=10000` or
    /// `depth=4`.
    pub fn parse(text: &str) -> Option<TimeControl> {
        if let Some(n) = text.strip_prefix("nodes=") {
            return n.parse().ok().map(TimeControl::Nodes);
        }
        if let Some(d) = text.strip_prefix("depth=") {
            return d.parse().ok().map(TimeControl::Depth);
        }
        if let Some((moves, seconds)) = text.split_once('/') {
            let moves = moves.parse::<u32>().ok().filter(|&m| m > 0)?;
            let seconds = seconds.parse::<f64>().ok().filter(|&s| s > 0.0)?;
            return Some(TimeControl::Repeating {
                moves,
                base: Duration::from_secs_f64(seconds),
            });
        }
        let (base, increment) = text.split_once('+').unwrap_or((text, "0"));
        let seconds = |s: &str| s.parse::<f64>().ok().filter(|v| *v >= 0.0);
        Some(TimeControl::Clock {
            base: Duration::from_secs_f64(seconds(base)?),
            increment: Duration::from_secs_f64(seconds(increment)?),
        })
    }
}

/// How a game ended, from White's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// White won.
    WhiteWins,
    /// Black won.
    BlackWins,
    /// A draw.
    Draw,
    /// No result: the game couldn't start. It must not be scored.
    Aborted,
}

impl Outcome {
    fn win_for(color: Color) -> Outcome {
        match color {
            Color::White => Outcome::WhiteWins,
            Color::Black => Outcome::BlackWins,
        }
    }

    /// The PGN result tag: `1-0`, `0-1`, `1/2-1/2`, or `*` (no result) for an aborted game.
    pub fn pgn(self) -> &'static str {
        match self {
            Outcome::WhiteWins => "1-0",
            Outcome::BlackWins => "0-1",
            Outcome::Draw => "1/2-1/2",
            Outcome::Aborted => "*",
        }
    }
}

/// How a game ended, for spotting engine problems in long test runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Termination {
    /// By the rules: mate, a draw rule, or adjudication.
    Normal,
    /// An engine ran out of time.
    TimeLoss,
    /// An engine sent an illegal move.
    IllegalMove,
    /// An engine crashed, hung or couldn't start the game (then the outcome is
    /// [`Outcome::Aborted`]).
    Failure,
}

/// A finished game.
#[derive(Clone, Debug)]
pub struct Game {
    /// The starting position.
    pub start_fen: String,
    /// The engines' names, White first.
    pub players: [String; 2],
    /// The moves played, in SAN.
    pub moves: Vec<String>,
    /// The result.
    pub outcome: Outcome,
    /// Why the game ended, like "checkmate" or "Black lost on time".
    pub reason: String,
    /// What kind of ending it was.
    pub termination: Termination,
    /// The side whose engine ended the game by a time loss, an illegal move or a failure;
    /// `None` when it ended by the rules.
    pub at_fault: Option<Color>,
}

impl fmt::Display for Game {
    /// The game in PGN.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "[White \"{}\"]", self.players[0])?;
        writeln!(f, "[Black \"{}\"]", self.players[1])?;
        writeln!(f, "[Result \"{}\"]", self.outcome.pgn())?;
        writeln!(f, "[FEN \"{}\"]", self.start_fen)?;
        writeln!(f, "[SetUp \"1\"]")?;
        writeln!(f, "[Termination \"{}\"]", self.reason)?;
        writeln!(f)?;
        let start = Position::from_fen(&self.start_fen).map_err(|_| fmt::Error)?;
        let mut number = start.fullmove_number();
        let mut white_to_move = start.side_to_move() == Color::White;
        let mut text = String::new();
        for (i, san) in self.moves.iter().enumerate() {
            if white_to_move {
                text.push_str(&format!("{number}. "));
            } else if i == 0 {
                text.push_str(&format!("{number}... "));
            }
            text.push_str(san);
            text.push(' ');
            if !white_to_move {
                number += 1;
            }
            white_to_move = !white_to_move;
        }
        text.push_str(self.outcome.pgn());
        // PGN lines should stay under 80 characters.
        let mut line = String::new();
        for word in text.split(' ') {
            if line.len() + word.len() + 1 > 79 {
                writeln!(f, "{}", line.trim_end())?;
                line.clear();
            }
            line.push_str(word);
            line.push(' ');
        }
        writeln!(f, "{}", line.trim_end())
    }
}

/// Games longer than this many plies are adjudicated as draws.
pub const MAX_PLIES: usize = 600;

/// Extra time an engine may use beyond its clock before losing on time, to absorb the cost of
/// starting the search and passing messages.
pub const TIME_MARGIN: Duration = Duration::from_millis(50);

/// Plays one game from `start_fen`; `engines[0]` plays White.
///
/// # Panics
///
/// If `start_fen` isn't a valid FEN: callers check their openings first, as
/// [`run_match`](crate::match_runner::run_match) does.
pub fn play_game(engines: [&mut UciEngine; 2], start_fen: &str, tc: TimeControl) -> Game {
    play_game_watched(engines, start_fen, tc, &mut |_, _, _| {})
}

/// Called after each move: the position, the moves so far in SAN, and the clocks (White's,
/// Black's) when the game has them.
pub type OnMove<'a> = dyn FnMut(&Position, &[String], Option<[Duration; 2]>) + 'a;

/// [`play_game`], calling `on_move` after each move with the position, the moves so far (in
/// SAN) and the clocks (White's, Black's) when the game has them.
///
/// # Panics
///
/// If `start_fen` isn't a valid FEN, as [`play_game`].
pub fn play_game_watched(
    engines: [&mut UciEngine; 2],
    start_fen: &str,
    tc: TimeControl,
    on_move: &mut OnMove<'_>,
) -> Game {
    let players = [engines[0].name.clone(), engines[1].name.clone()];
    let mut engines = engines;
    let mut game = rules::Game::from_fen(start_fen).expect("opening FENs are valid");
    let mut uci_moves: Vec<String> = Vec::new();
    let mut san_moves: Vec<String> = Vec::new();
    let mut clocks = match tc {
        TimeControl::Clock { base, .. } | TimeControl::Repeating { base, .. } => [base, base],
        _ => [Duration::ZERO; 2],
    };
    // Moves each side has made, for repeating time controls.
    let mut moves_made = [0u32; 2];

    // `at_fault` is the side to move for every ending but the rules' own.
    let finish = |termination: Termination,
                  outcome: Outcome,
                  reason: String,
                  moves: Vec<String>,
                  at_fault: Option<Color>| Game {
        start_fen: start_fen.to_string(),
        players: players.clone(),
        moves,
        outcome,
        reason,
        termination,
        at_fault,
    };

    for (engine, color) in engines.iter_mut().zip([Color::White, Color::Black]) {
        if let Err(e) = engine.new_game() {
            let name = &players[color.index()];
            return finish(
                Termination::Failure,
                Outcome::Aborted,
                format!("aborted: {color} ({name}) couldn't start the game: {e}"),
                san_moves,
                Some(color),
            );
        }
    }

    loop {
        // The rules first: is the game already over?
        if let Some(over) = game.outcome() {
            let outcome = over.winner().map_or(Outcome::Draw, Outcome::win_for);
            return finish(
                Termination::Normal,
                outcome,
                over.to_string(),
                san_moves,
                None,
            );
        }
        let side = game.position().side_to_move();
        if san_moves.len() >= MAX_PLIES {
            return finish(
                Termination::Normal,
                Outcome::Draw,
                "adjudicated: game too long".into(),
                san_moves,
                None,
            );
        }

        // Ask the engine to move.
        let mover = side.index();
        let position_cmd = if uci_moves.is_empty() {
            format!("position fen {start_fen}")
        } else {
            format!("position fen {start_fen} moves {}", uci_moves.join(" "))
        };
        let (go, timeout) = match tc {
            TimeControl::Clock { increment, .. } => {
                let ms = |d: Duration| d.as_millis();
                (
                    format!(
                        "go wtime {} btime {} winc {} binc {}",
                        ms(clocks[0]),
                        ms(clocks[1]),
                        ms(increment),
                        ms(increment)
                    ),
                    clocks[mover] + TIME_MARGIN + Duration::from_secs(1),
                )
            }
            TimeControl::Repeating { moves, .. } => {
                let ms = |d: Duration| d.as_millis();
                (
                    format!(
                        "go wtime {} btime {} movestogo {}",
                        ms(clocks[0]),
                        ms(clocks[1]),
                        moves - moves_made[mover] % moves
                    ),
                    clocks[mover] + TIME_MARGIN + Duration::from_secs(1),
                )
            }
            TimeControl::Nodes(n) => (format!("go nodes {n}"), Duration::from_secs(60)),
            TimeControl::Depth(d) => (format!("go depth {d}"), Duration::from_secs(300)),
        };
        let name = &players[mover];
        let reply = match engines[mover].go(&position_cmd, &go, timeout) {
            Ok(reply) => reply,
            Err(e) => {
                return finish(
                    Termination::Failure,
                    Outcome::win_for(!side),
                    format!("{side} ({name}) failed: {e}"),
                    san_moves,
                    Some(side),
                );
            }
        };
        let added = match tc {
            TimeControl::Clock { increment, .. } => Some(increment),
            TimeControl::Repeating { moves, base } => {
                moves_made[mover] += 1;
                // A new period starts after every `moves` moves.
                Some(if moves_made[mover] % moves == 0 {
                    base
                } else {
                    Duration::ZERO
                })
            }
            _ => None,
        };
        if let Some(added) = added {
            if reply.elapsed > clocks[mover] + TIME_MARGIN {
                // How far over it went tells an engine that thinks too long (a little over)
                // from a stalled process (far over).
                return finish(
                    Termination::TimeLoss,
                    Outcome::win_for(!side),
                    format!(
                        "{side} ({name}) lost on time at move {}: took {} ms with {} ms on its clock",
                        san_moves.len() / 2 + 1,
                        reply.elapsed.as_millis(),
                        clocks[mover].as_millis()
                    ),
                    san_moves,
                    Some(side),
                );
            }
            clocks[mover] = clocks[mover].saturating_sub(reply.elapsed) + added;
        }
        let Some(mv) = game.position().parse_uci_move(&reply.uci) else {
            return finish(
                Termination::IllegalMove,
                Outcome::win_for(!side),
                format!("{side} ({name}) played an illegal move: {}", reply.uci),
                san_moves,
                Some(side),
            );
        };
        let played = game
            .play(mv)
            .expect("the move was parsed as legal, and the game isn't over");
        san_moves.push(played.san.clone());
        uci_moves.push(reply.uci);
        let clocks = matches!(tc, TimeControl::Clock { .. }).then_some(clocks);
        on_move(game.position(), &san_moves, clocks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_time_controls() {
        assert_eq!(
            TimeControl::parse("8+0.08"),
            Some(TimeControl::Clock {
                base: Duration::from_secs(8),
                increment: Duration::from_millis(80)
            })
        );
        assert_eq!(
            TimeControl::parse("60"),
            Some(TimeControl::Clock {
                base: Duration::from_secs(60),
                increment: Duration::ZERO
            })
        );
        assert_eq!(
            TimeControl::parse("nodes=5000"),
            Some(TimeControl::Nodes(5000))
        );
        assert_eq!(TimeControl::parse("depth=3"), Some(TimeControl::Depth(3)));
        assert_eq!(
            TimeControl::parse("40/60"),
            Some(TimeControl::Repeating {
                moves: 40,
                base: Duration::from_secs(60)
            })
        );
        assert_eq!(TimeControl::parse("0/60"), None);
        assert_eq!(TimeControl::parse("fast"), None);
    }

    #[test]
    fn writes_pgn() {
        let game = Game {
            start_fen: cb2026_chess::STARTING_FEN.to_string(),
            players: ["A".into(), "B".into()],
            moves: ["f3", "e5", "g4", "Qh4#"].map(String::from).to_vec(),
            outcome: Outcome::BlackWins,
            reason: "checkmate".into(),
            termination: Termination::Normal,
            at_fault: None,
        };
        let pgn = game.to_string();
        assert!(pgn.contains("[Result \"0-1\"]"));
        assert!(pgn.ends_with("1. f3 e5 2. g4 Qh4# 0-1\n"), "{pgn}");
    }

    #[test]
    fn an_aborted_game_has_no_result() {
        let game = Game {
            start_fen: cb2026_chess::STARTING_FEN.to_string(),
            players: ["A".into(), "B".into()],
            moves: Vec::new(),
            outcome: Outcome::Aborted,
            reason: "aborted: White (A) couldn't start the game: the engine exited".into(),
            termination: Termination::Failure,
            at_fault: Some(Color::White),
        };
        let pgn = game.to_string();
        assert!(pgn.contains("[Result \"*\"]"), "{pgn}");
        assert!(pgn.ends_with("\n*\n"), "{pgn}");
    }
}
