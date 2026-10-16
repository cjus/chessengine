//! Plays a rating gauntlet: one engine against several reference engines with known ratings,
//! then estimates the engine's rating on their scale (see `cb2026_tools::gauntlet`).
//!
//! ```text
//! cargo run --release -p cb2026-tools --bin gauntlet -- \
//!     --engine target/release/cb2026 \
//!     --opponent /path/to/engine-a 2150 --opponent /path/to/engine-b 2400 ... \
//!     [--games 800] [--tc 8+0.08] [--concurrency 3] [--openings tests/data/openings.epd] \
//!     [--option Name=Value ...] [--pgn games.pgn]
//! ```
//!
//! `--games` is per opponent. After each opponent it lists the games that ended badly (time
//! losses, illegal moves, crashes, games that couldn't start) for each side. Exit status: 0
//! when the gauntlet ran, 2 for usage errors or a match that couldn't be played in full.

use std::fs::File;
use std::io::Write;
use std::process::ExitCode;

use cb2026_tools::game::TimeControl;
use cb2026_tools::gauntlet::{GauntletResult, RatingFit, fit_rating};
use cb2026_tools::match_runner::{MatchSettings, Problems, parse_openings, run_match};

const USAGE: &str = "Usage: gauntlet --engine PATH --opponent PATH RATING [--opponent PATH RATING ...] \
[--games N per opponent] [--tc 8+0.08|nodes=N|depth=N] [--concurrency N] [--openings FILE] \
[--option Name=Value] [--pgn FILE]";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("gauntlet: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut engine = None;
    let mut opponents: Vec<(String, f64)> = Vec::new();
    let mut tc = TimeControl::parse("8+0.08").expect("valid default");
    let mut games = 800;
    let mut concurrency = 1;
    let mut openings_path = "tests/data/openings.epd".to_string();
    let mut options = Vec::new();
    let mut pgn: Option<File> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--engine" => engine = Some(value()?),
            "--opponent" => {
                let path = value()?;
                let rating = args
                    .next()
                    .ok_or("--opponent needs a path and a rating")?
                    .parse::<f64>()
                    .map_err(|e| format!("--opponent {path}: rating: {e}"))?;
                opponents.push((path, rating));
            }
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
            "--option" => {
                let text = value()?;
                let (name, val) = text
                    .split_once('=')
                    .ok_or(format!("--option expects Name=Value, got '{text}'"))?;
                options.push((name.to_string(), val.to_string()));
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
    let engine = engine.ok_or("give the engine to rate with --engine")?;
    if opponents.is_empty() {
        return Err("give at least one --opponent".to_string());
    }
    let text =
        std::fs::read_to_string(&openings_path).map_err(|e| format!("{openings_path}: {e}"))?;
    let openings = parse_openings(&text);

    println!(
        "Gauntlet: {engine} against {} opponents, {games} games each, {tc:?}, {concurrency} at a time",
        opponents.len()
    );
    let mut results = Vec::new();
    for (path, rating) in &opponents {
        println!("\n{path} (rated {rating}):");
        let settings = MatchSettings {
            engines: [engine.clone(), path.clone()],
            options: options.clone(),
            first_options: Vec::new(),
            time_control: tc,
            openings: openings.clone(),
            max_games: games,
            concurrency,
            sprt: None,
        };
        let mut name = path.clone();
        let mut problems = Problems::default();
        let result = run_match(&settings, &mut |report| {
            problems.record(report);
            if let Some(file) = pgn.as_mut() {
                let _ = writeln!(file, "{}", report.game);
            }
            name = report.game.players[usize::from(report.first_engine_white)].clone();
            let n = report.score.games();
            if n % 50 == 0 {
                println!("  {}", report.score);
            }
        });
        let score = match result {
            Ok(score) => score,
            Err(e) => {
                // Not a usage error, so no usage text: just what went wrong.
                eprintln!("gauntlet: {path}: {e}");
                return Ok(ExitCode::from(2));
            }
        };
        println!("  Final: {score}");
        for (side, counts) in [&engine, &name].into_iter().zip(problems.counts) {
            println!("  Problems for {side}: {counts}");
        }
        for reason in problems.reasons.iter().take(10) {
            println!("    {reason}");
        }
        results.push(GauntletResult {
            name,
            rating: *rating,
            score,
        });
    }

    println!("\nResults:");
    for r in &results {
        let performance = r
            .score
            .elo()
            .map_or("-".to_string(), |(elo, _)| format!("{:.0}", r.rating + elo));
        println!(
            "  {:<32} rated {:>6.0}: +{} -{} ={}  ({:.1}%), performance {performance}",
            r.name,
            r.rating,
            r.score.wins,
            r.score.losses,
            r.score.draws,
            100.0 * r.score.ratio()
        );
    }
    match fit_rating(&results) {
        Some(RatingFit::Rating(rating, margin)) => {
            println!("Rating: {rating:.0} ± {margin:.0} (95%)")
        }
        Some(RatingFit::Above(bound)) => {
            println!("Rating: above {bound:.0} (won every game; add stronger opponents)")
        }
        Some(RatingFit::Below(bound)) => {
            println!("Rating: below {bound:.0} (lost every game; add weaker opponents)")
        }
        None => println!("Rating: no games played"),
    }
    Ok(ExitCode::SUCCESS)
}
