//! Counts how often cb2026's strength levels make inaccuracies, mistakes and blunders, as
//! Lichess counts them, to compare the levels with people (cjus/chessgui#113).
//!
//! ```text
//! cargo run --release -p cb2026-tools --bin mistakes -- \
//!     [--levels 0,3,6,9,12,15,18] [--games 40] [--nodes 200000] [--threads 1] \
//!     [--random-plies 8] [--seed 1] [--openings tests/data/openings.epd]
//!
//! curl -s https://database.lichess.org/standard/lichess_db_standard_rated_2025-01.pgn.zst \
//!     | zstdcat | cargo run --release -p cb2026-tools --bin mistakes -- \
//!     --people [--games 30000] [--band 100] [--speed rapid]
//! ```
//!
//! Each level plays itself from the openings, and a full-strength search of `--nodes` nodes
//! judges every move. With `--people`, it counts people's moves instead, in Lichess's games
//! read from standard input, by rating band, up to `--games` games with evaluations (the
//! download stops when it has read them). Both tables give counts for 40 moves of one player.
//! See `cb2026_tools::mistakes`.

use std::process::ExitCode;
use std::time::Instant;

use cb2026_engine::levels;
use cb2026_tools::match_runner::parse_openings;
use cb2026_tools::mistakes::{MistakeSettings, count_people, measure};
use cb2026_tools::selfplay::DatagenSettings;

const USAGE: &str = "Usage: mistakes [--levels L,L,...] [--games N] [--nodes N] [--threads N] \
[--random-plies N] [--seed N] [--openings FILE]\n       mistakes --people [--games N] \
[--band N] [--speed S] < games.pgn";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("mistakes: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut levels = vec![0, 3, 6, 9, 12, 15, 18];
    let mut games = 40;
    let mut people = false;
    let mut band = 100;
    let mut speed = "rapid".to_string();
    let mut nodes = 200_000;
    let mut threads = 1;
    let mut openings = DatagenSettings {
        games: 0,
        nodes: 0,
        random_plies: 8,
        threads: 1,
        seed: 1,
        openings: Vec::new(),
    };
    let mut openings_path = "tests/data/openings.epd".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let number = |text: String| text.parse::<u64>().map_err(|e| format!("{text}: {e}"));
        match arg.as_str() {
            "--levels" => {
                levels = value()?
                    .split(',')
                    .map(|level| {
                        level
                            .trim()
                            .parse::<u32>()
                            .map_err(|e| format!("{level}: {e}"))
                    })
                    .collect::<Result<_, _>>()?;
            }
            "--games" => games = number(value()?)? as usize,
            "--people" => people = true,
            "--band" => band = number(value()?)?.max(1) as u32,
            "--speed" => speed = value()?,
            "--nodes" => nodes = number(value()?)?,
            "--threads" => threads = number(value()?)? as usize,
            "--random-plies" => openings.random_plies = number(value()?)? as usize,
            "--seed" => openings.seed = number(value()?)?,
            "--openings" => openings_path = value()?,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    if people {
        let (tallies, evaluated) = count_people(std::io::stdin().lock(), games, band);
        println!(
            "People at {speed}, per 40 moves of one player, from {evaluated} evaluated games:"
        );
        println!("rating   moves  inaccuracies  mistakes  blunders");
        for ((kind, rating), tally) in &tallies {
            if *kind != speed {
                continue;
            }
            let [inaccuracies, mistakes, blunders] = tally.per_game();
            println!(
                "{rating:>6}  {:>6}  {inaccuracies:>12.2}  {mistakes:>8.2}  {blunders:>8.2}",
                tally.moves
            );
        }
        return Ok(());
    }
    let text = std::fs::read_to_string(&openings_path)
        .map_err(|e| format!("can't read {openings_path}: {e}"))?;
    openings.openings = parse_openings(&text);
    if openings.openings.is_empty() {
        return Err(format!("no openings in {openings_path}"));
    }

    println!("Per 40 moves of one player, judged at {nodes} nodes, {games} games a level:");
    println!("level  rating  moves  inaccuracies  mistakes  blunders  people's blunders");
    for level in levels {
        let start = Instant::now();
        let settings = MistakeSettings {
            level,
            games,
            judge_nodes: nodes,
            threads,
            openings: openings.clone(),
        };
        let tally = measure(&settings, &|_, _| {});
        let [inaccuracies, mistakes, blunders] = tally.per_game();
        // What the level is fitted to: people of its rating (none for full strength).
        let people = levels::PEOPLE_BLUNDERS
            .get(level as usize)
            .map_or("-".to_string(), |b| format!("{b:.1}"));
        println!(
            "{level:>5}  {:>6}  {:>5}  {inaccuracies:>12.1}  {mistakes:>8.1}  {blunders:>8.1}  \
             {people:>17}   ({:.0} s)",
            levels::RATINGS[level.min(levels::FULL_STRENGTH) as usize],
            tally.moves,
            start.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
