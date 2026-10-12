//! `match` and `selfplay`: short matches from inside the shell.
//!
//! `match <engine> <games> [tc]` plays cb2026, exactly as set up in the shell (level,
//! techniques switched off, hash size...), against another UCI engine. Our side runs as a UCI
//! session on a thread in this process, so it goes through the same referee
//! (`cb2026_tools::game`) as the `match` tool. Colours alternate, each opening is played once
//! with each colour, and the result comes with an Elo estimate. `selfplay <games> [tc]` plays
//! the shell's engine against a fresh copy of itself at default settings.
//!
//! It's for quick, friendly comparisons and for learners ("does my change beat the previous
//! stage?"). Rigorous testing uses the `match` tool's SPRT (see `docs/testing.md`).

use std::fs::OpenOptions;
use std::io::Write;

use cb2026_chess::Position;
use cb2026_chess::pgn::{PgnGame, split_games};
use cb2026_engine::{Engine, id_name};
use cb2026_tools::game::{Outcome, TimeControl, play_game_watched};
use cb2026_tools::match_runner::{DEFAULT_OPENINGS, parse_openings};
use cb2026_tools::sprt::Score;
use cb2026_tools::uci_client::UciEngine;

use crate::{Response, Shell, render};

/// The time control when none is given.
const DEFAULT_TC: &str = "10+0.1";
/// Where games are saved when no file is given.
const DEFAULT_PGN: &str = "cb2026-match.pgn";
/// The most games one command plays.
const MAX_GAMES: usize = 1000;

const USAGE: &str = "Usage: match <engine path> <games> [time control] [chess960] [openings <file.epd|file.pgn>] [pgn <file>]
       selfplay <games> [time control] [chess960] [openings <file>] [pgn <file>]
Time controls: 10+0.1 (seconds + increment, the default), nodes=20000, depth=6.";

/// What a `match` or `selfplay` command asked for.
struct MatchRequest {
    /// The opponent's path, or `None` for a copy of our own engine.
    opponent: Option<String>,
    games: usize,
    time_control: TimeControl,
    chess960: bool,
    openings: Option<String>,
    pgn: String,
}

fn parse_request(args: &str, selfplay: bool) -> Option<MatchRequest> {
    let mut words = args.split_whitespace();
    let opponent = if selfplay {
        None
    } else {
        Some(words.next()?.to_string())
    };
    let games = words
        .next()?
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=MAX_GAMES).contains(n))?;
    let mut request = MatchRequest {
        opponent,
        games,
        time_control: TimeControl::parse(DEFAULT_TC).expect("the default parses"),
        chess960: false,
        openings: None,
        pgn: DEFAULT_PGN.to_string(),
    };
    while let Some(word) = words.next() {
        match word {
            "chess960" => request.chess960 = true,
            "openings" => request.openings = Some(words.next()?.to_string()),
            "pgn" => request.pgn = words.next()?.to_string(),
            tc => request.time_control = TimeControl::parse(tc)?,
        }
    }
    Some(request)
}

/// Opening positions (FEN) from a file: EPD lines, or the final positions of PGN games.
fn read_openings(path: &str) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let epd: Vec<String> = parse_openings(&text)
        .into_iter()
        .filter(|fen| Position::from_fen(fen).is_ok())
        .collect();
    if !epd.is_empty() {
        return Ok(epd);
    }
    let mut fens = Vec::new();
    for game in split_games(&text) {
        let game = PgnGame::parse(game).map_err(|e| format!("{path}: {e}"))?;
        let mut pos = game.start.clone();
        for &mv in &game.moves {
            pos.make_move(mv);
        }
        fens.push(pos.to_fen());
    }
    if fens.is_empty() {
        return Err(format!("{path}: no positions (expected EPD or PGN)"));
    }
    Ok(fens)
}

impl Shell {
    /// Runs our engine, as set up in the shell, as an in-process UCI engine.
    fn our_engine(
        &self,
        engine: Engine,
        name: &str,
        options: &[(String, String)],
    ) -> Result<UciEngine, String> {
        UciEngine::in_process(name, options, move |input, output| {
            let _ = cb2026_protocol::uci::run_with_engine(input, output, None, engine);
        })
        .map_err(|e| format!("{name}: {e}"))
    }

    /// `match <engine> <games> [tc] ...` and `selfplay <games> [tc] ...`.
    pub(crate) fn play_match(
        &mut self,
        args: &str,
        selfplay: bool,
        progress: &mut dyn FnMut(&str),
    ) -> Response {
        let Some(request) = parse_request(args, selfplay) else {
            return Response::text(USAGE);
        };
        let openings = match (&request.openings, request.chess960) {
            (Some(path), _) => match read_openings(path) {
                Ok(openings) => openings,
                Err(e) => return Response::text(format!("No match: {e}")),
            },
            (None, true) => {
                // A random Chess960 position for each pair of games.
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos());
                (0..request.games.div_ceil(2))
                    .map(|i| {
                        let n = (seed as usize).wrapping_add(i * 389) % 960;
                        Position::chess960(n as u32).expect("below 960").to_fen()
                    })
                    .collect()
            }
            (None, false) => parse_openings(DEFAULT_OPENINGS),
        };
        let options: Vec<(String, String)> = if request.chess960 {
            vec![("UCI_Chess960".to_string(), "true".to_string())]
        } else {
            Vec::new()
        };

        let our_name = format!("{} (shell)", id_name());
        let mut ours = match self.our_engine(self.engine.clone(), &our_name, &options) {
            Ok(engine) => engine,
            Err(e) => return Response::text(format!("No match: {e}")),
        };
        let theirs = match &request.opponent {
            Some(path) => UciEngine::start(path, &options).map_err(|e| format!("{path}: {e}")),
            None => self.our_engine(Engine::new(), &format!("{} (default)", id_name()), &options),
        };
        let mut theirs = match theirs {
            Ok(engine) => engine,
            Err(e) => return Response::text(format!("No match: {e}")),
        };
        let their_name = theirs.name.clone();
        progress(&format!(
            "{our_name} vs {their_name}: {} games, saved to {}\n",
            request.games, request.pgn
        ));

        let mut score = Score::default();
        let mut saved = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&request.pgn)
            .map_err(|e| format!("{}: {e}", request.pgn));
        if let Err(e) = &saved {
            progress(&format!("Games won't be saved: {e}\n"));
        }
        let style = self.style;
        let interactive = self.interactive;
        for index in 0..request.games {
            let opening = &openings[(index / 2) % openings.len()];
            let ours_white = index % 2 == 0;
            let header = format!(
                "Game {} of {}: {} - {}   (score so far +{} -{} ={})",
                index + 1,
                request.games,
                if ours_white { &our_name } else { &their_name },
                if ours_white { &their_name } else { &our_name },
                score.wins,
                score.losses,
                score.draws
            );
            let mut on_move =
                |pos: &Position, moves: &[String], clocks: Option<[std::time::Duration; 2]>| {
                    if !interactive {
                        return;
                    }
                    // The side to move now didn't make the last move.
                    let last = moves.last().map(|san| match pos.side_to_move() {
                        cb2026_chess::Color::Black => format!("{}. {san}", pos.fullmove_number()),
                        cb2026_chess::Color::White => {
                            format!("{}... {san}", pos.fullmove_number().saturating_sub(1))
                        }
                    });
                    let mut screen = format!("\x1b[H\x1b[2J{header}\n\n");
                    screen.push_str(&render::board(pos, style, last.as_deref()));
                    if let Some([white, black]) = clocks {
                        screen.push_str(&format!(
                            "Clocks: White {:.1} s · Black {:.1} s\n",
                            white.as_secs_f64(),
                            black.as_secs_f64()
                        ));
                    }
                    progress(&screen);
                };
            let game = if ours_white {
                play_game_watched(
                    [&mut ours, &mut theirs],
                    opening,
                    request.time_control,
                    &mut on_move,
                )
            } else {
                play_game_watched(
                    [&mut theirs, &mut ours],
                    opening,
                    request.time_control,
                    &mut on_move,
                )
            };
            let ours_won = match game.outcome {
                Outcome::Draw => None,
                Outcome::WhiteWins => Some(ours_white),
                Outcome::BlackWins => Some(!ours_white),
            };
            match ours_won {
                Some(true) => score.wins += 1,
                Some(false) => score.losses += 1,
                None => score.draws += 1,
            }
            if let Ok(file) = saved.as_mut()
                && let Err(e) = writeln!(file, "{game}")
            {
                progress(&format!("Couldn't save game {}: {e}\n", index + 1));
            }
            progress(&format!(
                "Game {}: {} - {} {} ({}). Score: +{} -{} ={}\n",
                index + 1,
                game.players[0],
                game.players[1],
                game.outcome.pgn(),
                game.reason,
                score.wins,
                score.losses,
                score.draws
            ));
        }

        let points = f64::from(score.wins) + f64::from(score.draws) / 2.0;
        let mut out = format!(
            "Match over: {our_name} scored {points}/{} against {their_name} (+{} -{} ={}).",
            score.games(),
            score.wins,
            score.losses,
            score.draws
        );
        match score.elo() {
            Some((elo, margin)) => out.push_str(&format!(
                "\nElo difference: {elo:+.0} ± {margin:.0} (95%). A few games say little: the margin shows how little."
            )),
            None => out.push_str("\nNo Elo estimate: one side won (or drew) every game."),
        }
        if saved.is_ok() {
            out.push_str(&format!("\nGames saved to {}.", request.pgn));
        }
        Response::text(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_requests() {
        let r = parse_request("/usr/bin/engine 10", false).unwrap();
        assert_eq!(r.opponent.as_deref(), Some("/usr/bin/engine"));
        assert_eq!(r.games, 10);
        assert_eq!(r.time_control, TimeControl::parse(DEFAULT_TC).unwrap());
        let r = parse_request("4 depth=3 chess960 pgn out.pgn", true).unwrap();
        assert_eq!(r.opponent, None);
        assert_eq!(r.time_control, TimeControl::Depth(3));
        assert!(r.chess960);
        assert_eq!(r.pgn, "out.pgn");
        assert!(parse_request("engine", false).is_none());
        assert!(parse_request("engine 0", false).is_none());
        assert!(parse_request("engine 2 fast", false).is_none());
        assert!(parse_request("2 openings", true).is_none());
    }

    #[test]
    fn selfplay_plays_and_scores() {
        let mut shell = crate::tests::ascii_shell_for_tests();
        let path = std::env::temp_dir().join(format!("cb2026-selfplay-{}.pgn", std::process::id()));
        let path = path.to_str().unwrap();
        let mut lines = Vec::new();
        let out = shell
            .play_match(&format!("2 depth=1 pgn {path}"), true, &mut |l| {
                lines.push(l.to_string())
            })
            .output;
        assert!(out.starts_with("Match over:"), "{out}");
        assert!(out.contains("/2 against"), "{out}");
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("Game ")).count(),
            2,
            "{lines:?}"
        );
        let saved = std::fs::read_to_string(path).unwrap();
        assert_eq!(saved.matches("[White ").count(), 2);
        std::fs::remove_file(path).unwrap();
        let missing = shell
            .play_match("/no/such/engine 2", false, &mut |_| {})
            .output;
        assert!(
            missing.starts_with("No match: /no/such/engine"),
            "{missing}"
        );
    }
}
