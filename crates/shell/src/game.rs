//! Playing complete games in the shell: clocks, hints, resigning and draws, taking moves back
//! and forward, and saving and loading games as PGN. The game itself, and whether it is over,
//! is a [`Game`] from `cb2026-chess`.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cb2026_chess::Color;
use cb2026_chess::game::Game;
use cb2026_chess::pgn::PgnGame;
use cb2026_engine::{Limits, Score, id_name};

use crate::{Response, Shell, pv_san, score_text};

/// The engine accepts a draw offer when it doesn't think it's better than this (centipawns,
/// from its own side).
const DRAW_ACCEPT_CP: i32 = 0;

/// The chess clocks, when the game is played with them (`time 5+3`).
#[derive(Clone, Debug)]
pub(crate) struct Clock {
    base: Duration,
    increment: Duration,
    /// Time left for White and Black, as of `since`.
    remaining: [Duration; 2],
    /// When the side to move's clock started running.
    since: Instant,
}

impl Clock {
    fn new(base: Duration, increment: Duration) -> Clock {
        Clock {
            base,
            increment,
            remaining: [base; 2],
            since: Instant::now(),
        }
    }

    /// Starts again with full clocks.
    pub(crate) fn restart(&mut self) {
        *self = Clock::new(self.base, self.increment);
    }

    /// Time left for `side`, counting the running clock.
    fn left(&self, side: Color, to_move: Color) -> Duration {
        let left = self.remaining[side.index()];
        if side == to_move {
            left.saturating_sub(self.since.elapsed())
        } else {
            left
        }
    }
}

/// `m:ss` (or `m:ss.s` under ten seconds, when it matters).
fn clock_text(time: Duration) -> String {
    let seconds = time.as_secs_f64();
    let minutes = (seconds / 60.0).floor();
    if seconds < 10.0 {
        format!("0:{seconds:04.1}")
    } else {
        format!("{minutes:.0}:{:02.0}", (seconds - minutes * 60.0).floor())
    }
}

/// Today's date as PGN writes it, `YYYY.MM.DD` (UTC).
fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}.{month:02}.{day:02}")
}

impl Shell {
    /// Charges the time since the last move to the side that just moved (before `mv` is made),
    /// adds its increment, and records a loss on time if its clock ran out.
    pub(crate) fn run_clock(&mut self) {
        let mover = self.game.position().side_to_move();
        let Some(clock) = self.clock.as_mut() else {
            return;
        };
        let used = clock.since.elapsed();
        let left = &mut clock.remaining[mover.index()];
        if used > *left {
            *left = Duration::ZERO;
            self.game.lose_on_time(mover);
        } else {
            *left = *left - used + clock.increment;
        }
        clock.since = Instant::now();
    }

    /// Search limits from the clock, when the game has one.
    pub(crate) fn clock_limits(&self) -> Option<Limits> {
        let clock = self.clock.as_ref()?;
        let to_move = self.game.position().side_to_move();
        Some(Limits {
            time: [
                Some(clock.left(Color::White, to_move)),
                Some(clock.left(Color::Black, to_move)),
            ],
            increment: [Some(clock.increment); 2],
            ..Limits::default()
        })
    }

    /// The clocks, for the board display.
    pub(crate) fn clock_line(&self) -> Option<String> {
        let clock = self.clock.as_ref()?;
        let to_move = self.game.position().side_to_move();
        Some(format!(
            "Clocks: White {} · Black {}",
            clock_text(clock.left(Color::White, to_move)),
            clock_text(clock.left(Color::Black, to_move)),
        ))
    }

    /// `time <minutes>+<seconds>`, `time off`, or `time` to show the clocks.
    pub(crate) fn time_control(&mut self, args: &str) -> Response {
        let usage = "Usage: time <minutes>+<increment seconds> (like 5+3) | time off";
        match args {
            "" => match self.clock_line() {
                Some(line) => Response::text(line),
                None => Response::text("No clocks. Start them with `time 5+3`."),
            },
            "off" => {
                self.clock = None;
                Response::text("Clocks off.")
            }
            _ => {
                let (base, increment) = args.split_once('+').unwrap_or((args, "0"));
                let value = |s: &str| s.trim().parse::<f64>().ok().filter(|v| *v >= 0.0);
                match (value(base), value(increment)) {
                    (Some(minutes), Some(seconds)) if minutes > 0.0 => {
                        self.clock = Some(Clock::new(
                            Duration::from_secs_f64(minutes * 60.0),
                            Duration::from_secs_f64(seconds),
                        ));
                        Response::text(format!(
                            "Clocks set: {minutes} minutes each, plus {seconds} seconds per move. The engine plays to the clock."
                        ))
                    }
                    _ => Response::text(usage),
                }
            }
        }
    }

    /// `redo`: plays the last move taken back.
    pub(crate) fn redo(&mut self) -> Response {
        if let Some(over) = self.game_over() {
            return Response::text(format!("The game is over: {over}"));
        }
        match self.future.last().copied() {
            Some(mv) if self.game.position().legal_moves().contains(&mv) => {
                self.make_move(mv, None);
                self.show()
            }
            _ => Response::text("Nothing to redo."),
        }
    }

    /// `hint`: the engine's suggestion for the side to move.
    pub(crate) fn hint(&mut self) -> Response {
        if let Some(over) = self.game_over() {
            return Response::text(format!("The game is over: {over}"));
        }
        let mut engine = self.lab_engine();
        let limits = self.default_limits();
        let (info, mv, _) = self.lab_search(&mut engine, &limits);
        match (mv, info) {
            (Some(mv), Some(info)) => Response::text(format!(
                "Hint: {} ({}, depth {}): {}",
                self.game.position().to_san(mv),
                score_text(info.score),
                info.depth,
                pv_san(self.game.position(), &info.pv)
            )),
            _ => Response::text("No hint: no legal moves."),
        }
    }

    /// `resign`: the side to move gives up.
    pub(crate) fn resign(&mut self) -> Response {
        if let Some(over) = self.game_over() {
            return Response::text(format!("The game is over: {over}"));
        }
        self.game.resign(self.game.position().side_to_move());
        Response::text(self.game_over().unwrap_or_default())
    }

    /// `draw`: the side to move offers a draw. The engine, if it plays the other side, accepts
    /// when it doesn't think it's better.
    pub(crate) fn draw(&mut self) -> Response {
        if let Some(over) = self.game_over() {
            return Response::text(format!("The game is over: {over}"));
        }
        let us = self.game.position().side_to_move();
        if self.engine_plays[(!us).index()] {
            let mut engine = self.lab_engine();
            let limits = self.default_limits();
            let (info, _, _) = self.lab_search(&mut engine, &limits);
            // The search scores the position for us; the engine sees the opposite.
            let engine_view = match info.map(|i| i.score) {
                Some(Score::Centipawns(cp)) => -cp,
                Some(Score::Mate(moves)) if moves > 0 => -cb2026_engine::search::MATE,
                Some(Score::Mate(_)) => cb2026_engine::search::MATE,
                None => 0,
            };
            if engine_view > DRAW_ACCEPT_CP {
                return Response::text(format!(
                    "{} declines the draw: it thinks it's better ({}).",
                    cb2026_engine::NAME,
                    crate::pawns(engine_view)
                ));
            }
        }
        self.game.agree_draw();
        Response::text(self.game_over().unwrap_or_default())
    }

    /// The game so far as PGN.
    fn to_pgn(&self) -> PgnGame {
        let mut pgn = self.game.to_pgn();
        // A side is the engine's if the engine plays it, or made all of its moves (with `go`):
        // only the engine's moves carry a comment.
        let player = |color: Color| {
            let mut moves = self
                .game
                .moves()
                .iter()
                .filter(|p| p.before.side_to_move() == color)
                .peekable();
            let all_engine = moves.peek().is_some() && moves.all(|p| p.comment.is_some());
            if self.engine_plays[color.index()] || all_engine {
                id_name()
            } else {
                "Player".to_string()
            }
        };
        pgn.set_tag("Event", "Game in the cb2026 shell");
        pgn.set_tag("Site", "cb2026 shell");
        pgn.set_tag("Date", &today());
        pgn.set_tag("Round", "-");
        pgn.set_tag("White", &player(Color::White));
        pgn.set_tag("Black", &player(Color::Black));
        if let Some(clock) = &self.clock {
            pgn.set_tag(
                "TimeControl",
                &format!("{}+{}", clock.base.as_secs(), clock.increment.as_secs_f64()),
            );
        }
        if let Some(over) = self.game_over() {
            pgn.set_tag("Termination", &over);
        }
        pgn
    }

    /// `pgn`, `pgn save <file>`, `pgn load <file>`.
    pub(crate) fn pgn(&mut self, args: &str) -> Response {
        let (action, path) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
        let path = path.trim();
        match (action, path.is_empty()) {
            ("", _) => Response::text(self.to_pgn().to_pgn().trim_end()),
            ("save", false) => match std::fs::write(path, self.to_pgn().to_pgn()) {
                Ok(()) => Response::text(format!(
                    "Saved the game ({} moves) to {path}.",
                    self.game.moves().len()
                )),
                Err(e) => Response::text(format!("Couldn't save to {path}: {e}")),
            },
            ("load", false) => {
                let pgn = match std::fs::read_to_string(path)
                    .map_err(|e| e.to_string())
                    .and_then(|text| PgnGame::parse(&text))
                {
                    Ok(pgn) => pgn,
                    Err(e) => return Response::text(format!("Couldn't load {path}: {e}")),
                };
                match Game::from_pgn(&pgn) {
                    Ok(game) => self.set_game(game),
                    Err(e) => return Response::text(format!("Couldn't load {path}: {e}")),
                }
                let mut response = self.show();
                response.output.insert_str(
                    0,
                    &format!(
                        "Loaded {} vs {}, {} moves.\n",
                        pgn.tag("White").unwrap_or("?"),
                        pgn.tag("Black").unwrap_or("?"),
                        pgn.moves.len()
                    ),
                );
                response
            }
            _ => Response::text("Usage: pgn | pgn save <file> | pgn load <file>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_clocks_read_well() {
        let date = today();
        assert_eq!(date.len(), 10);
        assert!(date.starts_with("20"), "{date}");
        assert_eq!(clock_text(Duration::from_secs(300)), "5:00");
        assert_eq!(clock_text(Duration::from_secs(65)), "1:05");
        assert_eq!(clock_text(Duration::from_millis(9_420)), "0:09.4");
    }
}
