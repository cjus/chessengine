//! A UCI session never panics or hangs, whatever the GUI sends.
//!
//! Searches are kept tiny so the fuzzer spends its time on the parser: every `go` gets a
//! node limit, the hash table is small (allocating the default 16 MB each run would be most
//! of the work), and the session ends at the end of the input.

#![no_main]

use std::io::{self, Cursor};
use std::sync::OnceLock;

use cb2026_engine::Engine;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let script: String = text
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("go") {
                format!("{line} nodes 64\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    // One engine with a 1 MB table, copied for each run.
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    let engine = ENGINE
        .get_or_init(|| {
            let mut engine = Engine::new();
            engine.set_hash_size(1);
            engine
        })
        .clone();
    let _ = cb2026_protocol::uci::run_with_engine(
        Cursor::new(script),
        Box::new(io::sink()),
        None,
        engine,
    );
});
