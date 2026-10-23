//! Shows what a file of NNUE training records holds.
//!
//! ```text
//! cargo run --release -p cb2026-tools --bin records -- FILE [--show N]
//! ```
//!
//! Checks every record and prints how many there are, how the games ended, how the scores
//! spread and who is to move, then the first `N` records (default 5) as positions with their
//! score, result and best move. Exits with status 1 if a record is damaged or the file's length
//! isn't a whole number of records. `datagen --format records` writes these files (see
//! `cb2026_nnue::record`).

use std::fs::File;
use std::io::{BufReader, ErrorKind, Read};
use std::process::ExitCode;

use cb2026_chess::Color;
use cb2026_nnue::{GameResult, RECORD_LEN, Record};

const USAGE: &str = "Usage: records FILE [--show N]";

/// Scores within this many centipawns of zero count as "level".
const LEVEL: i32 = 100;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("records: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut path = None;
    let mut show = 5usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--show" => {
                let value = args
                    .next()
                    .ok_or(format!("--show needs a value\n{USAGE}"))?;
                show = value.parse().map_err(|e| format!("--show: {e}"))?;
            }
            _ if path.is_none() && !arg.starts_with("--") => path = Some(arg),
            other => return Err(format!("unknown argument '{other}'\n{USAGE}")),
        }
    }
    let path = path.ok_or(USAGE)?;
    let file = File::open(&path).map_err(|e| format!("can't open {path}: {e}"))?;
    let mut reader = BufReader::new(file);

    let mut count = 0u64;
    let mut results = [0u64; 3];
    let mut black_to_move = 0u64;
    let mut level = 0u64;
    let mut total_score = 0u64;
    let mut first = Vec::new();
    let mut bytes = [0u8; RECORD_LEN];
    loop {
        match read_record(&mut reader, &mut bytes) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => return Err(format!("{path}: after {count} records: {e}")),
        }
        let record =
            Record::from_bytes(&bytes).map_err(|e| format!("{path}: record {}: {e}", count + 1))?;
        count += 1;
        results[match record.result() {
            GameResult::WhiteWin => 0,
            GameResult::Draw => 1,
            GameResult::BlackWin => 2,
        }] += 1;
        black_to_move += u64::from(record.side_to_move() == Color::Black);
        let score = i32::from(record.score());
        level += u64::from(score.abs() <= LEVEL);
        total_score += u64::from(score.unsigned_abs());
        if first.len() < show {
            first.push(record);
        }
    }

    println!(
        "{path}: {count} records ({} bytes)",
        count * RECORD_LEN as u64
    );
    if count == 0 {
        return Ok(());
    }
    let share = |n: u64| 100.0 * n as f64 / count as f64;
    println!(
        "results: White won {:.1}%, drawn {:.1}%, Black won {:.1}%",
        share(results[0]),
        share(results[1]),
        share(results[2])
    );
    println!(
        "scores (White's side): mean size {:.0} cp; {:.1}% within ±{LEVEL} cp",
        total_score as f64 / count as f64,
        share(level)
    );
    println!(
        "to move: White {:.1}%, Black {:.1}%",
        share(count - black_to_move),
        share(black_to_move)
    );
    for record in &first {
        let pos = record.to_position();
        let result = match record.result() {
            GameResult::WhiteWin => "1-0",
            GameResult::Draw => "1/2",
            GameResult::BlackWin => "0-1",
        };
        println!(
            "  {}  score {:+}  {result}  best {}",
            pos.to_fen(),
            record.score(),
            pos.to_san(record.best_move())
        );
    }
    Ok(())
}

/// Reads the next record into `bytes`: `Ok(false)` at the end of the file, an error if the
/// file ends partway through a record.
fn read_record(reader: &mut impl Read, bytes: &mut [u8; RECORD_LEN]) -> Result<bool, String> {
    let mut filled = 0;
    while filled < RECORD_LEN {
        match reader.read(&mut bytes[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(format!("the file ends {filled} bytes into a record")),
            Ok(n) => filled += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(true)
}
