//! Plays a match between two UCI engines and reports the score, Elo and SPRT verdict.
//!
//! ```text
//! cargo run --release -p cb2026-tools --bin match -- \
//!     --engine NEW --engine OLD \
//!     [--tc 8+0.08 | --tc 40/60 | --tc nodes=20000 | --tc depth=4] \
//!     [--games 1000] [--concurrency 3] [--openings tests/data/openings.epd] \
//!     [--sprt 0 5] [--option Name=Value ...] [--first-option Name=Value ...] [--pgn games.pgn]
//! ```
//!
//! `--chess960` plays from Chess960 starting positions (and sets `UCI_Chess960` in both
//! engines) instead of the opening file.
//!
//! `--option` is sent to both engines, `--first-option` only to the first (for example
//! `--first-option Threads=4` to measure how the engine scales against one thread).
//!
//! Results are from the first engine's point of view. The last lines count the games that
//! ended badly (time losses, illegal moves, crashes), for robustness runs. With
//! `--sprt elo0 elo1` the match stops as soon as the test passes or fails. Exit status: 0 if
//! the SPRT passed (or no SPRT was run), 1 if it failed or was inconclusive, 2 for usage
//! errors.
//! as soon as the test passes or fails. Exit status: 0 if the SPRT passed (or no SPRT was
//! run), 1 if it failed or was inconclusive, 2 for usage errors.

use std::fs::File;
use std::io::Write;
use std::process::ExitCode;

use cb2026_chess::Position;
use cb2026_tools::game::{Termination, TimeControl};
use cb2026_tools::match_runner::{MatchSettings, parse_openings, run_match};
use cb2026_tools::sprt::{Sprt, Verdict};

const USAGE: &str = "Usage: match --engine A --engine B [--tc 8+0.08|40/60|nodes=N|depth=N] \
[--games N] [--concurrency N] [--openings FILE] [--sprt ELO0 ELO1] [--option Name=Value] \
[--first-option Name=Value] [--chess960] [--pgn FILE]";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("match: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut engines = Vec::new();
    let mut tc = TimeControl::parse("8+0.08").expect("valid default");
    let mut games = 1000;
    let mut concurrency = 1;
    let mut openings_path = "tests/data/openings.epd".to_string();
    let mut chess960 = false;
    let mut sprt = None;
    let mut options = Vec::new();
    let mut first_options = Vec::new();
    let mut pgn: Option<File> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--engine" => engines.push(value()?),
            "--tc" => {
                let text = value()?;
                tc = TimeControl::parse(&text).ok_or(format!("bad time control '{text}'"))?;
            }
            "--games" => games = value()?.parse().map_err(|e| format!("--games: {e}"))?,
            "--concurrency" => {
                concurrency = value()?
                    .parse()
                    .map_err(|e| format!("--concurrency: {e}"))?
            }
            "--openings" => openings_path = value()?,
            "--chess960" => chess960 = true,
            "--sprt" => {
                let elo0: f64 = value()?.parse().map_err(|e| format!("--sprt: {e}"))?;
                let elo1: f64 = args
                    .next()
                    .ok_or("--sprt needs two values")?
                    .parse()
                    .map_err(|e| format!("--sprt: {e}"))?;
                sprt = Some(Sprt::new(elo0, elo1));
            }
            "--option" | "--first-option" => {
                let text = value()?;
                let (name, val) = text
                    .split_once('=')
                    .ok_or(format!("{arg} expects Name=Value, got '{text}'"))?;
                let list = if arg == "--option" {
                    &mut options
                } else {
                    &mut first_options
                };
                list.push((name.to_string(), val.to_string()));
            }
            "--pgn" => {
                let path = value()?;
                pgn = Some(File::create(&path).map_err(|e| format!("{path}: {e}"))?);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(ExitCode::SUCCESS);
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    let engines: [String; 2] = engines
        .try_into()
        .map_err(|_| "give exactly two --engine paths".to_string())?;
    let openings = if chess960 {
        // Chess960 start positions, spread over the 960 (389 is coprime with 960, so the
        // first 960 games use each position once).
        options.push(("UCI_Chess960".to_string(), "true".to_string()));
        (0..960u32)
            .map(|k| {
                Position::chess960(k * 389 % 960)
                    .expect("valid number")
                    .to_fen()
            })
            .collect()
    } else {
        let text =
            std::fs::read_to_string(&openings_path).map_err(|e| format!("{openings_path}: {e}"))?;
        parse_openings(&text)
    };
    let settings = MatchSettings {
        engines,
        options,
        first_options,
        time_control: tc,
        openings,
        max_games: games,
        concurrency,
        sprt,
    };

    println!(
        "{} vs {}, {:?}, up to {} games, {} at a time",
        settings.engines[0], settings.engines[1], tc, settings.max_games, concurrency
    );
    if let Some(sprt) = sprt {
        let (lower, upper) = sprt.bounds();
        println!(
            "SPRT: H0 elo {} vs H1 elo {}, bounds [{lower:.2}, {upper:.2}]",
            sprt.elo0, sprt.elo1
        );
    }
    let mut last_verdict = None;
    // Games that ended badly, by engine (0 = first) and kind.
    let mut problems: Vec<(usize, Termination, String)> = Vec::new();
    let score = run_match(&settings, &mut |report| {
        if report.game.termination != Termination::Normal {
            // The side that lost caused the problem.
            let loser_white = report.game.outcome == cb2026_tools::game::Outcome::BlackWins;
            let first = usize::from(loser_white != report.first_engine_white);
            problems.push((first, report.game.termination, report.game.reason.clone()));
        }
        if let Some(file) = pgn.as_mut() {
            let _ = writeln!(file, "{}", report.game);
        }
        last_verdict = report.verdict;
        let n = report.score.games();
        let finished = matches!(report.verdict, Some(Verdict::Pass | Verdict::Fail));
        if n % 10 == 0 || finished {
            let llr = sprt.map_or(String::new(), |s| {
                format!("  LLR {:.2}", report.score.llr(s.elo0, s.elo1))
            });
            println!("{}{llr}", report.score);
        }
    })?;

    println!("Final: {score}");
    for (which, name) in settings.engines.iter().enumerate() {
        let count = |kind: Termination| {
            problems
                .iter()
                .filter(|(w, k, _)| *w == which && *k == kind)
                .count()
        };
        println!(
            "Problems for {name}: {} time losses, {} illegal moves, {} failures",
            count(Termination::TimeLoss),
            count(Termination::IllegalMove),
            count(Termination::Failure)
        );
    }
    for (_, _, reason) in problems.iter().take(10) {
        println!("  {reason}");
    }
    let code = match last_verdict {
        Some(Verdict::Pass) => {
            println!("SPRT: H1 accepted (passed)");
            ExitCode::SUCCESS
        }
        Some(Verdict::Fail) => {
            println!("SPRT: H0 accepted (failed)");
            ExitCode::FAILURE
        }
        Some(Verdict::Continue) => {
            println!("SPRT: inconclusive after {} games", score.games());
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    };
    Ok(code)
}
