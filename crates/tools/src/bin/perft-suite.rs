//! Runs perft suites and reports correctness and speed.
//!
//! ```text
//! cargo run --release -p cb2026-tools --bin perft-suite -- [--max-nodes N] [suite.epd ...]
//! ```
//!
//! With no files it runs `tests/data/perft.epd` and `tests/data/perft960.epd`. Depths whose
//! expected count is above `--max-nodes` are skipped (default: run everything). Exits with
//! status 1 if any count is wrong.

use std::process::ExitCode;
use std::time::Instant;

use cb2026_chess::Position;
use cb2026_chess::perft::{parse_suite, perft};

const DEFAULT_SUITES: [&str; 2] = ["tests/data/perft.epd", "tests/data/perft960.epd"];

fn main() -> ExitCode {
    let mut max_nodes = u64::MAX;
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--max-nodes" {
            match args.next().and_then(|n| n.parse().ok()) {
                Some(n) => max_nodes = n,
                None => {
                    eprintln!("--max-nodes needs a number");
                    return ExitCode::from(2);
                }
            }
        } else {
            files.push(arg);
        }
    }
    if files.is_empty() {
        files = DEFAULT_SUITES.iter().map(|s| s.to_string()).collect();
    }

    let mut failures = 0;
    let mut total_nodes = 0u64;
    let start = Instant::now();
    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("{file}: {e}");
                return ExitCode::from(2);
            }
        };
        println!("{file}");
        for case in parse_suite(&text) {
            let pos = match Position::from_fen(&case.fen) {
                Ok(pos) => pos,
                Err(e) => {
                    println!("  FAIL  {}: {e}", case.fen);
                    failures += 1;
                    continue;
                }
            };
            println!("  {}", case.fen);
            for &(depth, expected) in case.counts.iter().filter(|(_, n)| *n <= max_nodes) {
                let t = Instant::now();
                let nodes = perft(&pos, depth);
                let secs = t.elapsed().as_secs_f64();
                total_nodes += nodes;
                let verdict = if nodes == expected { "ok  " } else { "FAIL" };
                if nodes != expected {
                    failures += 1;
                }
                println!(
                    "    {verdict} D{depth} {nodes:>12} (expected {expected:>12})  {:>7.1} Mnps",
                    nodes as f64 / secs.max(1e-9) / 1e6
                );
            }
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "\n{total_nodes} nodes in {secs:.1} s: {:.1} million nodes/s; {failures} failure(s)",
        total_nodes as f64 / secs.max(1e-9) / 1e6
    );
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
