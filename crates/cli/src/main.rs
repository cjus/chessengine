//! The `cb2026` binary.
//!
//! One executable serves every front end. A GUI starts it with no arguments and sends `uci` or
//! `xboard` as its first line, Solrac sends a Chess Node Protocol message (JSON), and a person
//! in a terminal gets the shell. Flags force a mode when guessing isn't wanted. See
//! `docs/PRD.md`, "One binary, three modes".

#![forbid(unsafe_code)]

mod args;

use std::io::{self, BufRead, IsTerminal, Read};
use std::process::ExitCode;

use args::{Command, Mode, Settings, USAGE};
use cb2026_engine::{BUILTIN_NETWORK_NAME, Engine, EvalChoice, Network, builtin_network};
use cb2026_protocol::Protocol;
use cb2026_shell::{Shell, Style};

/// Exit code for bad command-line arguments.
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&args) {
        Ok(Command::Version) => {
            // With the commit, so a build can be told from another of the same version.
            println!("{} {}", cb2026_engine::NAME, cb2026_engine::build_version());
            ExitCode::SUCCESS
        }
        Ok(Command::Help) => {
            println!("{}\n\n{USAGE}", cb2026_engine::id_name());
            ExitCode::SUCCESS
        }
        Ok(Command::Bench(net)) => {
            let mut engine = Engine::new();
            match EvalChoice::parse(net.as_deref().unwrap_or(BUILTIN_NETWORK_NAME)) {
                EvalChoice::BuiltIn => {}
                EvalChoice::HandCrafted => engine.network = None,
                EvalChoice::File(path) => match std::fs::read(path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| Network::from_bytes(&bytes).map_err(|e| e.to_string()))
                {
                    Ok(net) => engine.network = Some(std::sync::Arc::new(net)),
                    Err(e) => {
                        eprintln!("cb2026: {path}: {e}");
                        return ExitCode::from(EXIT_USAGE);
                    }
                },
            }
            // What it evaluates with, on stderr so the three lines below stay as tools expect.
            match &engine.network {
                Some(net) if std::sync::Arc::ptr_eq(net, &builtin_network()) => {
                    eprintln!("The built-in NNUE network, {}", net.summary());
                }
                Some(net) => eprintln!("NNUE network {}", net.summary()),
                None => eprintln!("The hand-crafted evaluation"),
            }
            let (nodes, time) =
                cb2026_engine::bench::bench(&mut engine, cb2026_engine::bench::BENCH_DEPTH);
            // The same layout as Stockfish and OpenBench use, so tools can read it.
            println!("Total time (ms) : {}", time.as_millis());
            println!("Nodes searched  : {nodes}");
            println!(
                "Nodes/second    : {:.0}",
                nodes as f64 / time.as_secs_f64().max(1e-9)
            );
            ExitCode::SUCCESS
        }
        Ok(Command::Run(mode, settings)) => run(mode, settings),
        Ok(Command::Serve(address, settings, network)) => serve(address, settings, network),
        Ok(Command::Pair(data_dir)) => pair(data_dir),
        Err(message) => {
            eprintln!("cb2026: {message}\n\n{USAGE}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

/// Starts the requested mode, resolving [`Mode::Auto`] first.
fn run(mode: Mode, settings: Settings) -> ExitCode {
    let (mode, first_line) = match mode {
        Mode::Auto => detect_mode(),
        forced => (forced, None),
    };
    match mode {
        Mode::Uci => run_uci(first_line),
        Mode::Xboard => run_xboard(first_line),
        Mode::Cnp => run_cnp(first_line, settings),
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
    // A Chess Node Protocol message is a JSON object; no UCI, XBoard or shell command starts
    // with `{`.
    if first_line.trim_start().starts_with('{') {
        return (Mode::Cnp, Some(first_line));
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

/// Speaks the Chess Node Protocol on standard input and output until `exit`: exit code 0 if
/// `shutdown` came first, 1 otherwise (the protocol's rule).
///
/// `settings` give the node's threads and hash size, which CNP has no option for: a node that
/// searches for a client on another machine is started with what that machine should spend.
fn run_cnp(first_line: Option<String>, settings: Settings) -> ExitCode {
    // With Syzygy probing the binary links GPL code, so it is GPL as a whole (README,
    // "License"), and it says so to the client.
    let license = if cfg!(feature = "syzygy") {
        "GPL-3.0-or-later"
    } else {
        "MIT OR Apache-2.0"
    };
    let stdin = io::stdin();
    match cb2026_cnp::run(
        stdin.lock(),
        Box::new(io::stdout()),
        first_line,
        cnp_engine(settings),
        cb2026_cnp::Config {
            license,
            message_bytes: None,
        },
    ) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("cb2026: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Where the node's trust files are: the directory asked for, or the system's default.
#[cfg(feature = "net")]
fn trust_store(data_dir: Option<std::path::PathBuf>) -> Result<cb2026_net::Store, String> {
    data_dir
        .or_else(cb2026_net::default_dir)
        .map(cb2026_net::Store::new)
        .ok_or_else(|| {
            "this system has no home directory for cb2026's files; use --data-dir".into()
        })
}

/// Serves the Chess Node Protocol on the network until the program is stopped.
///
/// On a loopback address the listener is plain, for a program on this computer; on any other
/// (or with `--tls`) it is encrypted and serves only approved clients.
#[cfg(feature = "net")]
fn serve(address: std::net::SocketAddr, settings: Settings, network: args::Network) -> ExitCode {
    let license = if cfg!(feature = "syzygy") {
        "GPL-3.0-or-later"
    } else {
        "MIT OR Apache-2.0"
    };
    let mut config = cb2026_net::Config::new(address, license);
    let secure = network.tls || !address.ip().is_loopback();
    if secure {
        let opened = trust_store(network.data_dir)
            .and_then(|store| cb2026_net::Trust::open(store).map_err(|e| e.to_string()));
        match opened {
            Ok(trust) => config.trust = Some(std::sync::Arc::new(trust)),
            Err(e) => {
                eprintln!("cb2026: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let trust = config.trust.clone();
    let server = match cb2026_net::Server::bind(config) {
        Ok(server) => server,
        Err(e) => {
            eprintln!("cb2026: {e}");
            return ExitCode::FAILURE;
        }
    };
    // The address the system gave, which differs from the one asked for when the port was 0.
    let bound = match server.local_addr() {
        Ok(bound) => bound,
        Err(e) => {
            eprintln!("cb2026: {e}");
            return ExitCode::FAILURE;
        }
    };
    match &trust {
        None => eprintln!(
            "cb2026: serving the Chess Node Protocol on {bound}, one client at a time \
             (no encryption: loopback only). Stop it with Ctrl-C."
        ),
        Some(trust) => {
            eprintln!(
                "cb2026: serving the Chess Node Protocol on {bound}, one client at a time, \
                 encrypted, for approved clients only. Stop it with Ctrl-C."
            );
            eprintln!(
                "This node's fingerprint:\n{}",
                trust.fingerprint().in_lines()
            );
            eprintln!(
                "To approve a client, run `cb2026 --pair` on this computer (add `--data-dir` \
                 if you gave one here)."
            );
        }
    }
    match server.serve(|| cnp_engine(settings)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cb2026: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `--serve` in a build without the listener.
#[cfg(not(feature = "net"))]
fn serve(_address: std::net::SocketAddr, _settings: Settings, _network: args::Network) -> ExitCode {
    no_listener()
}

/// `--pair` in a build without the listener.
#[cfg(not(feature = "net"))]
fn pair(_data_dir: Option<std::path::PathBuf>) -> ExitCode {
    no_listener()
}

#[cfg(not(feature = "net"))]
fn no_listener() -> ExitCode {
    eprintln!("cb2026: this build has no network listener (build with --features net)");
    ExitCode::from(EXIT_USAGE)
}

/// Opens a pairing window and waits for one client to use it.
///
/// Shows the node's fingerprint, which the person compares with what the client shows, and the
/// one-time code, which the person types into the client. The listener (`--serve`, a program of
/// its own) sees the open window in the shared files; this waits until a client has paired, or
/// the window has closed.
#[cfg(feature = "net")]
fn pair(data_dir: Option<std::path::PathBuf>) -> ExitCode {
    use std::time::{Duration, SystemTime};

    let run = || -> Result<ExitCode, String> {
        let store = trust_store(data_dir)?;
        let identity = store.identity().map_err(|e| e.to_string())?;
        let known = store.approved().map_err(|e| e.to_string())?;
        let window = store
            .open_pairing(SystemTime::now())
            .map_err(|e| e.to_string())?;
        println!("Pairing: approve one new client.\n");
        println!("1. On the client, choose this computer. It shows a fingerprint; check that");
        println!("   it is the same as this one:\n");
        for line in identity.fingerprint().in_lines().lines() {
            println!("     {line}");
        }
        println!("\n2. Only if they match, type this code into the client:\n");
        println!("     {}\n", window.code_in_groups());
        println!(
            "The code works once and for {} minutes, and closes after {} wrong tries.",
            cb2026_net::trust::PAIRING_MINUTES,
            cb2026_net::trust::PAIRING_ATTEMPTS
        );
        println!("`cb2026 --serve` must be running on this computer. Waiting for the client...");
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = store.approved().map_err(|e| e.to_string())?;
            if let Some(added) = now
                .iter()
                .find(|a| !known.iter().any(|k| k.fingerprint == a.fingerprint))
            {
                println!("\nPaired: {} ({})", added.name, added.fingerprint);
                return Ok(ExitCode::SUCCESS);
            }
            if !store.pairing_is_open(SystemTime::now()) {
                println!(
                    "\nThe window closed and nobody paired (it ran out of time, or the code \
                     was wrong too often). Run `cb2026 --pair` again for a new code."
                );
                return Ok(ExitCode::FAILURE);
            }
        }
    };
    run().unwrap_or_else(|e| {
        eprintln!("cb2026: {e}");
        ExitCode::FAILURE
    })
}

/// A new engine for a CNP node, with the threads and hash size it was started with.
fn cnp_engine(settings: Settings) -> Engine {
    let mut engine = new_engine();
    if let Some(threads) = settings.threads {
        engine.threads = threads;
    }
    if let Some(megabytes) = settings.hash_mb {
        engine.set_hash_size(megabytes);
    }
    engine
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
    let mut input = io::Cursor::new(first_line.unwrap_or_default()).chain(stdin.lock());
    let switch = match cb2026_shell::run(&mut shell, &mut input, io::stdout().lock(), interactive) {
        Ok(switch) => switch,
        // A closed pipe (for example `cb2026 < script | head`) is a normal way to stop.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cb2026: {e}");
            return ExitCode::FAILURE;
        }
    };
    // `uci` or `xboard` typed in the shell: carry on in that protocol, with the shell's engine
    // and the rest of the input, as if the session had started with that command.
    let engine = shell.into_engine();
    let output = Box::new(io::stdout());
    let result = match switch {
        None => return ExitCode::SUCCESS,
        Some(Protocol::Uci) => {
            cb2026_protocol::uci::run_with_engine(input, output, Some("uci".to_string()), engine)
        }
        Some(Protocol::Xboard) => {
            cb2026_protocol::xboard::run(input, output, Some("xboard".to_string()), engine)
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cnp_engine_takes_the_threads_and_hash_it_was_started_with() {
        let engine = cnp_engine(Settings {
            threads: Some(6),
            hash_mb: Some(48),
        });
        assert_eq!(engine.threads, 6);
        assert_eq!(engine.tt.megabytes(), 48);
    }

    #[test]
    fn without_settings_a_cnp_engine_keeps_the_defaults() {
        let engine = cnp_engine(Settings::default());
        assert_eq!(engine.threads, new_engine().threads);
        assert_eq!(engine.tt.megabytes(), new_engine().tt.megabytes());
    }
}
