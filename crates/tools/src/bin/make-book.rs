//! Makes a Polyglot opening book from games in PGN read from standard input, such as
//! Lichess's (CC0), for cb2026's `BookFile` option or any program that reads Polyglot books.
//!
//! ```text
//! curl -s https://database.lichess.org/standard/lichess_db_standard_rated_2026-09.pgn.zst \
//!     | zstdcat | cargo run --release -p cb2026-tools --bin make-book -- --out lichess.bin \
//!     [--min-rating 2200] [--speeds blitz,rapid,classical] [--plies 20] [--min-games 20] \
//!     [--min-score 35] [--games N]
//! ```
//!
//! It uses the games where both players are rated `--min-rating` or more, at one of
//! `--speeds` (`any` for every game), with no bots, up to `--games` games (the download stops
//! when it has them). It counts the moves in their first `--plies` plies and keeps those played
//! in at least `--min-games` games that scored at least `--min-score` percent, weighted by
//! their points. Progress goes to standard error every million games read. See
//! `cb2026_tools::book_maker`.

use std::process::ExitCode;
use std::time::Instant;

use cb2026_chess::Position;
use cb2026_tools::book_maker::{BookSettings, count_games, shares};

const USAGE: &str = "Usage: make-book --out FILE [--min-rating N] [--speeds S,S,...|any] \
[--plies N] [--min-games N] [--min-score PERCENT] [--games N] < games.pgn";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("make-book: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut settings = BookSettings::default();
    let mut out = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let number = |text: String| text.parse::<u64>().map_err(|e| format!("{text}: {e}"));
        match arg.as_str() {
            "--out" => out = Some(value()?),
            "--min-rating" => settings.min_rating = number(value()?)? as u32,
            "--speeds" => {
                let speeds = value()?;
                settings.speeds = match speeds.as_str() {
                    "any" => Vec::new(),
                    _ => speeds.split(',').map(|s| s.trim().to_string()).collect(),
                };
            }
            "--plies" => settings.plies = number(value()?)? as usize,
            "--min-games" => settings.min_games = number(value()?)?.max(1) as u32,
            "--min-score" => settings.min_score = number(value()?)?.min(100) as f64 / 100.0,
            "--games" => settings.max_games = number(value()?)? as usize,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    let out = out.ok_or("--out is needed")?;

    let start = Instant::now();
    let counts = count_games(std::io::stdin().lock(), &settings, &mut |counts| {
        eprintln!(
            "{:>6.0} s  read {} games, used {}",
            start.elapsed().as_secs_f64(),
            counts.games_read,
            counts.games_used
        );
    });
    let book = counts.book(&settings);
    let bytes = book.to_bytes();
    std::fs::write(&out, &bytes).map_err(|e| format!("can't write {out}: {e}"))?;

    let speeds = match settings.speeds.is_empty() {
        true => "any speed".to_string(),
        false => settings.speeds.join(", "),
    };
    println!(
        "Read {} games and used {}: both players rated {} or more, {speeds}, no bots.",
        counts.games_read, counts.games_used, settings.min_rating
    );
    println!(
        "Counted {} moves in their first {} plies and kept {}: played in {} games or more, \
         scoring {:.0}% or more.",
        counts.moves.len(),
        settings.plies,
        book.len(),
        settings.min_games,
        100.0 * settings.min_score
    );
    println!(
        "Wrote {out}: {} entries, {:.1} MB, in {:.0} s.",
        book.len(),
        bytes.len() as f64 / 1e6,
        start.elapsed().as_secs_f64()
    );
    let first: Vec<String> = shares(&book, &Position::startpos())
        .iter()
        .map(|(san, share)| format!("{san} {:.0}%", 100.0 * share))
        .collect();
    println!("From the starting position: {}", first.join(", "));
    Ok(())
}
