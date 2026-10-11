//! Tunes the evaluation's parameters on self-play data (Texel's method) and writes them out
//! as Rust source.
//!
//! ```text
//! cargo run --release -p cb2026-tools --bin datagen -- --out data.txt
//! cargo run --release -p cb2026-tools --bin tune -- --data data.txt \
//!     [--epochs 3000] [--rate 1.0] [--lambda 1e-7] [--min-count 500] [--validation 0.1] \
//!     [--from hand|current] [--out crates/engine/src/tuned.rs]
//! ```
//!
//! The last `--validation` fraction of the data file (whole games, mostly, since each game's
//! positions are written together) is held out: its error shows whether the fit generalises
//! or just memorises the training positions.
//!
//! `--from hand` starts from the hand-written values (`EvalParams::hand_set`), `--from
//! current` (the default) from the values the engine uses now. See `cb2026_tools::texel`.

use std::process::ExitCode;
use std::time::Instant;

use cb2026_chess::Position;
use cb2026_engine::eval::EvalParams;
use cb2026_engine::tuning;
use cb2026_tools::selfplay::Sample;
use cb2026_tools::texel::{
    Adam, TrainingPosition, error, find_k, gradient, occurrences, regularize, rust_source,
};

const USAGE: &str = "Usage: tune --data FILE [--epochs N] [--rate R] [--lambda L] \
[--min-count N] [--validation F] [--from hand|current] [--out FILE]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("tune: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut data = None;
    let mut epochs = 3000;
    let mut rate = 1.0;
    let mut lambda = 1e-7;
    let mut min_count = 500;
    let mut validation = 0.1;
    let mut from_hand = false;
    let mut out = "crates/engine/src/tuned.rs".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--data" => data = Some(value()?),
            "--epochs" => epochs = value()?.parse().map_err(|e| format!("--epochs: {e}"))?,
            "--rate" => rate = value()?.parse().map_err(|e| format!("--rate: {e}"))?,
            "--lambda" => lambda = value()?.parse().map_err(|e| format!("--lambda: {e}"))?,
            "--validation" => {
                validation = value()?.parse().map_err(|e| format!("--validation: {e}"))?
            }
            "--min-count" => {
                min_count = value()?.parse().map_err(|e| format!("--min-count: {e}"))?
            }
            "--from" => from_hand = value()? == "hand",
            "--out" => out = value()?,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    let data = data.ok_or("--data is required")?;
    let params = if from_hand {
        EvalParams::hand_set()
    } else {
        EvalParams::default()
    };

    let text = std::fs::read_to_string(&data).map_err(|e| format!("can't read {data}: {e}"))?;
    let positions: Vec<TrainingPosition> = text
        .lines()
        .filter_map(Sample::from_line)
        .filter_map(|s| {
            let pos = Position::from_fen(&s.fen).ok()?;
            Some(TrainingPosition {
                features: tuning::features(&pos, &params),
                result: s.result,
            })
        })
        .collect();
    if positions.is_empty() {
        return Err(format!("no positions in {data}"));
    }
    let held_out = ((positions.len() as f64 * validation) as usize).min(positions.len() - 1);
    let (positions, check) = positions.split_at(positions.len() - held_out);
    println!(
        "{} training positions, {} held out for validation",
        positions.len(),
        check.len()
    );

    let mut values = tuning::values(&params);
    let start_values = values.clone();
    let seen = occurrences(positions, values.len());
    let frozen = seen.iter().filter(|&&n| n < min_count).count();
    println!("{frozen} parameters seen in fewer than {min_count} positions keep their values");
    let k = find_k(positions, &values);
    let start_error = error(positions, &values, k);
    let start_check = error(check, &values, k);
    println!("K = {k:.4}, error {start_error:.6}, validation {start_check:.6}");

    let start = Instant::now();
    let mut adam = Adam::new(values.len(), rate);
    for epoch in 1..=epochs {
        let mut grad = gradient(positions, &values, k);
        regularize(&mut grad, &values, &start_values, &seen, lambda, min_count);
        adam.step(&mut values, &grad);
        if epoch % 100 == 0 || epoch == epochs {
            println!(
                "epoch {epoch}: error {:.6}, validation {:.6} ({:.0} s)",
                error(positions, &values, k),
                error(check, &values, k),
                start.elapsed().as_secs_f64()
            );
        }
    }
    let end_error = error(positions, &values, k);
    let end_check = error(check, &values, k);

    let rounded: Vec<(i32, i32)> = values
        .iter()
        .map(|&(mg, eg)| (mg.round() as i32, eg.round() as i32))
        .collect();
    let notes = vec![
        format!(
            "Tuned on {} positions from `{}`, {epochs} epochs of Adam (rate {rate}, L2 {lambda});",
            positions.len(),
            std::path::Path::new(&data)
                .file_name()
                .map_or(data.clone(), |n| n.to_string_lossy().into_owned())
        ),
        format!(
            "parameters seen in fewer than {min_count} positions kept their {} values.",
            if from_hand { "hand-set" } else { "previous" }
        ),
        format!(
            "K = {k:.4}; training error {start_error:.6} -> {end_error:.6}, validation \
             {start_check:.6} -> {end_check:.6} ({} positions held out).",
            check.len()
        ),
    ];
    std::fs::write(&out, rust_source(&rounded, &notes))
        .map_err(|e| format!("can't write {out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}
