//! Plays cb2026 against itself and writes quiet positions with game results, for tuning and
//! NNUE training.
//!
//! ```text
//! cargo run --release -p cb2026-tools --bin datagen -- \
//!     --out data.txt [--format text|records] [--games 3000] [--nodes 6000] \
//!     [--random-plies 8] [--max-start-score N|off] [--threads 1] [--seed 1] \
//!     [--openings tests/data/openings.epd] [--evalfile builtin|none|FILE]
//! ```
//!
//! - `--format text` (the default), for the Texel tuner: each line is `FEN;result`, the result
//!   from White's side (1.0, 0.5 or 0.0).
//! - `--format records`, for the NNUE trainer: 32-byte records with the whole position, the
//!   search's score, the result and the best move (see `cb2026_nnue::record`). Files of records
//!   can be joined with `cat`; `records` shows what one holds.
//!
//! `--max-start-score N` draws a game's random opening moves again while its start scores beyond
//! `N` centipawns, so the games start roughly balanced; `off` keeps every start. The default is
//! 200 for records and `off` for text, so the tuner's runs in chapter 16 can still be repeated.
//!
//! `--evalfile` chooses what the games are played with, read as UCI's `EvalFile` is: the
//! built-in NNUE network (the default, so that the next network learns from the current one's
//! games), `none` for the hand-crafted evaluation (which played the first network's data and
//! chapter 16's), or a network file.
//!
//! See `cb2026_tools::selfplay` for how games are played and which positions are kept.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cb2026_engine::{EvalChoice, Network, builtin_network};
use cb2026_tools::match_runner::parse_openings;
use cb2026_tools::selfplay::{DatagenSettings, generate};

const USAGE: &str = "Usage: datagen --out FILE [--format text|records] [--games N] [--nodes N] \
[--random-plies N] [--max-start-score N|off] [--threads N] [--seed N] [--openings FILE] \
[--evalfile builtin|none|FILE]";

/// The largest start score, in centipawns, kept by default when writing records. Measured on
/// 600 games (2026-10-23): with no limit 18.5% of the records were within ±100 cp and 37.5%
/// beyond ±500; with 300, 24.4% and 27.9%; with 200, 30.0% and 25.3%, and White, draw and Black
/// results of 41, 23 and 37% instead of 54, 17 and 29%. Records came as fast either way.
const DEFAULT_MAX_START_SCORE: i32 = 200;

/// What `datagen` writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    /// `FEN;result` lines, for the Texel tuner.
    Text,
    /// 32-byte records, for the NNUE trainer.
    Records,
}

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
    let mut format = Format::Text;
    let mut settings = DatagenSettings {
        games: 3000,
        nodes: 6000,
        random_plies: 8,
        threads: 1,
        seed: 1,
        openings: Vec::new(),
        max_start_score: None,
        network: Some(builtin_network()),
    };
    let mut max_start_score = None;
    let mut openings_path = "tests/data/openings.epd".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let number = |text: String| text.parse::<u64>().map_err(|e| format!("{arg}: {e}"));
        match arg.as_str() {
            "--out" => out = Some(value()?),
            "--format" => {
                format = match value()?.as_str() {
                    "text" => Format::Text,
                    "records" => Format::Records,
                    other => return Err(format!("unknown format '{other}' (text or records)")),
                }
            }
            "--games" => settings.games = number(value()?)? as usize,
            "--nodes" => settings.nodes = number(value()?)?,
            "--random-plies" => settings.random_plies = number(value()?)? as usize,
            "--max-start-score" => {
                max_start_score = Some(match value()?.as_str() {
                    "off" => None,
                    text => Some(number(text.to_string())?.min(i32::MAX as u64) as i32),
                })
            }
            "--threads" => settings.threads = number(value()?)? as usize,
            "--seed" => settings.seed = number(value()?)?,
            "--openings" => openings_path = value()?,
            "--evalfile" => {
                settings.network = match EvalChoice::parse(&value()?) {
                    EvalChoice::BuiltIn => Some(builtin_network()),
                    EvalChoice::HandCrafted => None,
                    EvalChoice::File(path) => {
                        let bytes =
                            std::fs::read(path).map_err(|e| format!("can't read {path}: {e}"))?;
                        Some(Arc::new(
                            Network::from_bytes(&bytes).map_err(|e| format!("{path}: {e}"))?,
                        ))
                    }
                }
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    let out = out.ok_or("--out is required")?;
    settings.max_start_score = max_start_score.unwrap_or(match format {
        Format::Records => Some(DEFAULT_MAX_START_SCORE),
        Format::Text => None,
    });
    let text = std::fs::read_to_string(&openings_path)
        .map_err(|e| format!("can't read {openings_path}: {e}"))?;
    settings.openings = parse_openings(&text);
    if settings.openings.is_empty() {
        return Err(format!("no openings in {openings_path}"));
    }

    match &settings.network {
        Some(net) if Arc::ptr_eq(net, &builtin_network()) => {
            println!("Playing with the built-in NNUE network, {}", net.summary());
        }
        Some(net) => println!("Playing with NNUE network {}", net.summary()),
        None => println!("Playing with the hand-crafted evaluation"),
    }
    let file = File::create(&out).map_err(|e| format!("can't create {out}: {e}"))?;
    let writer = Mutex::new(BufWriter::new(file));
    let start = Instant::now();
    let positions = Mutex::new(0usize);
    let games = settings.games;
    // The first write error, kept for the end: a long run that fills the disk must fail, not
    // look like it worked.
    let failure: Mutex<Option<std::io::Error>> = Mutex::new(None);
    let fail = |e: std::io::Error| {
        failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert(e);
    };
    generate(&settings, &|index, game| {
        let mut writer = writer.lock().unwrap_or_else(|e| e.into_inner());
        let written = match format {
            Format::Text => game
                .samples()
                .iter()
                .try_for_each(|sample| writeln!(writer, "{}", sample.to_line())),
            Format::Records => game
                .records()
                .iter()
                .try_for_each(|record| writer.write_all(&record.to_bytes())),
        };
        if let Err(e) = written {
            fail(e);
        }
        let mut count = positions.lock().unwrap_or_else(|e| e.into_inner());
        *count += game.positions.len();
        let done = index + 1;
        if done % 100 == 0 || done == games {
            if let Err(e) = writer.flush() {
                fail(e);
            }
            println!(
                "{done}/{games} games, {} positions, {:.0} s",
                *count,
                start.elapsed().as_secs_f64()
            );
        }
    });
    if let Some(e) = failure.into_inner().unwrap_or_else(|e| e.into_inner()) {
        return Err(format!("writing {out}: {e}"));
    }
    let mut writer = writer.into_inner().unwrap_or_else(|e| e.into_inner());
    writer.flush().map_err(|e| format!("writing {out}: {e}"))?;
    Ok(())
}
