//! Playing one game between two UCI engines, as a GUI or tournament manager would.
//!
//! The referee (this code) keeps the real board and the clocks. Before each move it sends the
//! engine to move the full game (`position fen ... moves ...`) and the clocks (`go wtime ...`),
//! times the reply itself, checks that the move is legal, and ends the game on checkmate,
//! stalemate, the draw rules, a time loss, an illegal move or a crash.

use std::fmt;
use std::time::Duration;

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
    /// A fixed number of nodes per move: no clocks, results don't depend on machine load.
    Nodes(u64),
    /// A fixed depth per move.
    Depth(u32),
}

impl TimeControl {
    /// Parses `8+0.08` (seconds), `nodes=10000` or `depth=4`.
    pub fn parse(text: &str) -> Option<TimeControl> {
        if let Some(n) = text.strip_prefix("nodes=") {
            return n.parse().ok().map(TimeControl::Nodes);
        }
        if let Some(d) = text.strip_prefix("depth=") {
            return d.parse().ok().map(TimeControl::Depth);
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
}

impl Outcome {
    fn win_for(color: Color) -> Outcome {
        match color {
            Color::White => Outcome::WhiteWins,
            Color::Black => Outcome::BlackWins,
        }
    }

    /// The PGN result tag: `1-0`, `0-1` or `1/2-1/2`.
    pub fn pgn(self) -> &'static str {
        match self {
            Outcome::WhiteWins => "1-0",
            Outcome::BlackWins => "0-1",
            Outcome::Draw => "1/2-1/2",
        }
    }
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
pub fn play_game(engines: [&mut UciEngine; 2], start_fen: &str, tc: TimeControl) -> Game {
    let players = [engines[0].name.clone(), engines[1].name.clone()];
    let mut engines = engines;
    let mut position = Position::from_fen(start_fen).expect("opening FENs are valid");
    let mut uci_moves: Vec<String> = Vec::new();
    let mut san_moves: Vec<String> = Vec::new();
    let mut hashes = vec![position.hash()];
    let mut clocks = match tc {
        TimeControl::Clock { base, .. } => [base, base],
        _ => [Duration::ZERO; 2],
    };

    let finish = |outcome: Outcome, reason: String, moves: Vec<String>| Game {
        start_fen: start_fen.to_string(),
        players: players.clone(),
        moves,
        outcome,
        reason,
    };

    for engine in engines.iter_mut() {
        if let Err(e) = engine.new_game() {
            return finish(Outcome::Draw, format!("aborted: {e}"), san_moves);
        }
    }

    loop {
        // The rules first: is the game already over?
        let side = position.side_to_move();
        if position.legal_moves().is_empty() {
            return if position.in_check() {
                finish(Outcome::win_for(!side), "checkmate".into(), san_moves)
            } else {
                finish(Outcome::Draw, "stalemate".into(), san_moves)
            };
        }
        if position.halfmove_clock() >= 100 {
            return finish(Outcome::Draw, "fifty-move rule".into(), san_moves);
        }
        if position.is_insufficient_material() {
            return finish(Outcome::Draw, "insufficient material".into(), san_moves);
        }
        if hashes.iter().filter(|&&h| h == position.hash()).count() >= 3 {
            return finish(Outcome::Draw, "threefold repetition".into(), san_moves);
        }
        if san_moves.len() >= MAX_PLIES {
            return finish(
                Outcome::Draw,
                "adjudicated: game too long".into(),
                san_moves,
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
            TimeControl::Nodes(n) => (format!("go nodes {n}"), Duration::from_secs(60)),
            TimeControl::Depth(d) => (format!("go depth {d}"), Duration::from_secs(300)),
        };
        let name = &players[mover];
        let reply = match engines[mover].go(&position_cmd, &go, timeout) {
            Ok(reply) => reply,
            Err(e) => {
                return finish(
                    Outcome::win_for(!side),
                    format!("{side} ({name}) failed: {e}"),
                    san_moves,
                );
            }
        };
        if let TimeControl::Clock { increment, .. } = tc {
            if reply.elapsed > clocks[mover] + TIME_MARGIN {
                return finish(
                    Outcome::win_for(!side),
                    format!("{side} ({name}) lost on time"),
                    san_moves,
                );
            }
            clocks[mover] = clocks[mover].saturating_sub(reply.elapsed) + increment;
        }
        let Some(mv) = position.parse_uci_move(&reply.uci) else {
            return finish(
                Outcome::win_for(!side),
                format!("{side} ({name}) played an illegal move: {}", reply.uci),
                san_moves,
            );
        };
        san_moves.push(position.to_san(mv));
        uci_moves.push(reply.uci);
        position.make_move(mv);
        hashes.push(position.hash());
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
        };
        let pgn = game.to_string();
        assert!(pgn.contains("[Result \"0-1\"]"));
        assert!(pgn.ends_with("1. f3 e5 2. g4 Qh4# 0-1\n"), "{pgn}");
    }
}
