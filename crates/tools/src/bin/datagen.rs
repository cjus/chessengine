//! Plays cb2026 against itself and writes quiet positions with game results, for tuning.
//!
//! ```text
//! cargo run --release -p cb2026-tools --bin datagen -- \
//!     --out data.txt [--games 3000] [--nodes 6000] [--random-plies 8] [--threads 1] \
//!     [--seed 1] [--openings tests/data/openings.epd]
//! ```
//!
//! Each line of the output is `FEN;result`, the result from White's side (1.0, 0.5 or 0.0).
//! See `cb2026_tools::selfplay` for how games are played and which positions are kept.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::Instant;

use cb2026_tools::match_runner::parse_openings;
use cb2026_tools::selfplay::{DatagenSettings, generate};

const USAGE: &str = "Usage: datagen --out FILE [--games N] [--nodes N] [--random-plies N] \
[--threads N] [--seed N] [--openings FILE]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("datagen: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut out = None;
    let mut settings = DatagenSettings {
        games: 3000,
        nodes: 6000,
        random_plies: 8,
        threads: 1,
        seed: 1,
        openings: Vec::new(),
    };
    let mut openings_path = "tests/data/openings.epd".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let number = |text: String| text.parse::<u64>().map_err(|e| format!("{arg}: {e}"));
        match arg.as_str() {
            "--out" => out = Some(value()?),
            "--games" => settings.games = number(value()?)? as usize,
            "--nodes" => settings.nodes = number(value()?)?,
            "--random-plies" => settings.random_plies = number(value()?)? as usize,
            "--threads" => settings.threads = number(value()?)? as usize,
            "--seed" => settings.seed = number(value()?)?,
            "--openings" => openings_path = value()?,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    let out = out.ok_or("--out is required")?;
    let text = std::fs::read_to_string(&openings_path)
        .map_err(|e| format!("can't read {openings_path}: {e}"))?;
    settings.openings = parse_openings(&text);
    if settings.openings.is_empty() {
        return Err(format!("no openings in {openings_path}"));
    }

    let file = File::create(&out).map_err(|e| format!("can't create {out}: {e}"))?;
    let writer = Mutex::new(BufWriter::new(file));
    let start = Instant::now();
    let positions = Mutex::new(0usize);
    let games = settings.games;
    generate(&settings, &|index, samples| {
        let mut writer = writer.lock().unwrap_or_else(|e| e.into_inner());
        for sample in &samples {
            let _ = writeln!(writer, "{}", sample.to_line());
        }
        let mut count = positions.lock().unwrap_or_else(|e| e.into_inner());
        *count += samples.len();
        let done = index + 1;
        if done % 100 == 0 || done == games {
            let _ = writer.flush();
            println!(
                "{done}/{games} games, {} positions, {:.0} s",
                *count,
                start.elapsed().as_secs_f64()
            );
        }
    });
    let mut writer = writer.into_inner().unwrap_or_else(|e| e.into_inner());
    writer.flush().map_err(|e| format!("writing {out}: {e}"))?;
    Ok(())
}
