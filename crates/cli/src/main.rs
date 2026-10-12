//! The `cb2026` binary.
//!
//! One executable serves every front end. A GUI starts it with no arguments and sends `uci` or
//! `xboard` as its first line; a person in a terminal gets the shell. Flags force a mode when
//! guessing isn't wanted. See `docs/PRD.md`, "One binary, three modes".

#![forbid(unsafe_code)]

mod args;

use std::io::{self, BufRead, IsTerminal, Read};
use std::process::ExitCode;

use args::{Command, Mode, USAGE};
use cb2026_engine::Engine;
use cb2026_protocol::Protocol;
use cb2026_shell::{Shell, Style};

/// Exit code for bad command-line arguments.
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&args) {
        Ok(Command::Version) => {
            println!("{}", cb2026_engine::id_name());
            ExitCode::SUCCESS
        }
        Ok(Command::Help) => {
            println!("{}\n\n{USAGE}", cb2026_engine::id_name());
            ExitCode::SUCCESS
        }
        Ok(Command::Bench) => {
            let (nodes, time) = cb2026_engine::bench::bench(
                &mut cb2026_engine::Engine::new(),
                cb2026_engine::bench::BENCH_DEPTH,
            );
            // The same layout as Stockfish and OpenBench use, so tools can read it.
            println!("Total time (ms) : {}", time.as_millis());
            println!("Nodes searched  : {nodes}");
            println!(
                "Nodes/second    : {:.0}",
                nodes as f64 / time.as_secs_f64().max(1e-9)
            );
            ExitCode::SUCCESS
        }
        Ok(Command::Run(mode)) => run(mode),
        Err(message) => {
            eprintln!("cb2026: {message}\n\n{USAGE}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

/// Starts the requested mode, resolving [`Mode::Auto`] first.
fn run(mode: Mode) -> ExitCode {
    let (mode, first_line) = match mode {
        Mode::Auto => detect_mode(),
        forced => (forced, None),
    };
    match mode {
        Mode::Uci => run_uci(first_line),
        Mode::Xboard => run_xboard(first_line),
        Mode::Shell | Mode::Auto => run_shell(first_line),
    }
}

/// Picks a mode when none was forced, returning the first input line if it had to read one.
///
/// A terminal means a person: open the shell. Otherwise something started us with a pipe,
/// usually a GUI, and its first line says which protocol it speaks; anything else is treated
/// as a script of shell commands, starting with that line.
///
/// When the protocols are implemented, the first line will be handed to them as well, so the
/// handshake (`uci` → `id ...`, `uciok`) is still answered.
fn detect_mode() -> (Mode, Option<String>) {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return (Mode::Shell, None);
    }
    let mut first_line = String::new();
    if stdin.lock().read_line(&mut first_line).is_err() {
        return (Mode::Shell, None);
    }
    let mode = match cb2026_protocol::detect(&first_line) {
        Some(Protocol::Uci) => Mode::Uci,
        Some(Protocol::Xboard) => Mode::Xboard,
        None => Mode::Shell,
    };
    (mode, Some(first_line))
}

/// Speaks UCI on standard input and output until `quit`.
fn run_uci(first_line: Option<String>) -> ExitCode {
    let stdin = io::stdin();
    match cb2026_protocol::uci::run_with_engine(
        stdin.lock(),
        Box::new(io::stdout()),
        first_line,
        new_engine(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cb2026: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Speaks XBoard (CECP) on standard input and output until `quit`.
fn run_xboard(first_line: Option<String>) -> ExitCode {
    let stdin = io::stdin();
    match cb2026_protocol::xboard::run(
        stdin.lock(),
        Box::new(io::stdout()),
        first_line,
        new_engine(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cb2026: {e}");
            ExitCode::FAILURE
        }
    }
}

/// A new engine, able to open tablebases if this build includes them.
fn new_engine() -> Engine {
    let mut engine = Engine::new();
    #[cfg(feature = "syzygy")]
    {
        engine.tablebase_loader = Some(cb2026_syzygy::load);
    }
    engine
}

/// Runs the interactive shell (or a script of shell commands piped in).
fn run_shell(first_line: Option<String>) -> ExitCode {
    let stdin = io::stdin();
    let interactive = stdin.is_terminal() && io::stdout().is_terminal();
    let style = Style {
        unicode: !dumb_terminal(),
        color: io::stdout().is_terminal() && !no_color_requested() && !dumb_terminal(),
        flipped: false,
    };
    let mut shell = Shell::with_engine(style, new_engine());
    // A line already read while detecting the mode runs first, then the rest of the input.
    let input = io::Cursor::new(first_line.unwrap_or_default()).chain(stdin.lock());
    match cb2026_shell::run(&mut shell, input, io::stdout().lock(), interactive) {
        Ok(()) => ExitCode::SUCCESS,
        // A closed pipe (for example `cb2026 < script | head`) is a normal way to stop.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cb2026: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Whether the user asked for no colour (<https://no-color.org>: `NO_COLOR` set and non-empty).
fn no_color_requested() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// Whether the terminal says it can't do much (`TERM=dumb`): then use plain ASCII.
fn dumb_terminal() -> bool {
    std::env::var("TERM").is_ok_and(|t| t == "dumb")
}
