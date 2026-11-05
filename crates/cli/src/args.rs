//! Command-line arguments.
//!
//! Kept deliberately small and dependency-free: GUIs start the engine with no arguments at all,
//! so flags are only for people at a terminal and for scripts.

use cb2026_engine::MAX_THREADS;
use cb2026_engine::tt::MAX_HASH_MB;

/// What the user asked the binary to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Run in the given mode, with the given settings.
    Run(Mode, Settings),
    /// Run the fixed benchmark and print its node count and speed, evaluating with the NNUE
    /// network in this file, or with none (`none`, the hand-crafted evaluation), if given; with
    /// the built-in network otherwise.
    Bench(Option<String>),
    /// Print the engine name and version, with the commit it was built from.
    Version,
    /// Print usage.
    Help,
}

/// What the person starting the binary chose for the engine itself, as opposed to its mode.
///
/// A GUI sets the thread count and the hash size through its own protocol (UCI's `Threads`
/// and `Hash`); the Chess Node Protocol has no such option, so a person or script starting a
/// CNP node, on a machine that is to do the searching for someone else, chooses them here.
/// `None` leaves the engine's default (one thread, 16 MB).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Search threads, 1 to [`MAX_THREADS`].
    pub threads: Option<usize>,
    /// The transposition table's size in megabytes, 1 to [`MAX_HASH_MB`].
    pub hash_mb: Option<usize>,
}

/// How the binary talks to whoever started it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Decide from the input: a terminal gets the shell; otherwise the first line decides
    /// between UCI, XBoard, the Chess Node Protocol (a line of JSON) and shell commands (see
    /// [`cb2026_protocol::detect`]).
    Auto,
    /// The interactive shell.
    Shell,
    /// The UCI protocol.
    Uci,
    /// The XBoard / CECP protocol.
    Xboard,
    /// The Chess Node Protocol, which Solrac speaks (`cb2026_cnp`).
    Cnp,
}

/// Usage text for `--help` and argument errors.
pub const USAGE: &str = "\
Usage: cb2026 [COMMAND | OPTION]

With no arguments, cb2026 opens the shell in a terminal, or speaks UCI, XBoard or the Chess
Node Protocol when a program starts it (its first line, `uci`, `xboard` or a JSON message,
decides).

Commands:
  bench [NET]    Run the fixed benchmark; prints total nodes and speed. It evaluates with
                 the built-in NNUE network, or with NET: a .nnue file, or none for the
                 hand-crafted evaluation

Options:
  --shell        Force the interactive shell
  --uci          Force UCI mode
  --xboard       Force XBoard/CECP mode
  --cnp          Force Chess Node Protocol mode (JSON-RPC, one message per line)
  --threads N    With --cnp: search with N threads (1-256, default 1)
  --hash MB      With --cnp: use a transposition table of MB megabytes (1-65536, default 16)
  -V, --version  Print name and version
  -h, --help     Print this help";

/// Parses the arguments after the program name.
///
/// At most one command or mode is accepted; `--threads` and `--hash` go with `--cnp` and nothing
/// else. Anything else is an error with a message for the user.
pub fn parse(args: &[String]) -> Result<Command, String> {
    match args {
        [] => return Ok(Command::Run(Mode::Auto, Settings::default())),
        [command] if command == "bench" => return Ok(Command::Bench(None)),
        [command, net] if command == "bench" => return Ok(Command::Bench(Some(net.clone()))),
        _ => {}
    }
    let mut mode = None;
    let mut settings = Settings::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let chosen = match arg.as_str() {
            "--shell" => Mode::Shell,
            "--uci" => Mode::Uci,
            "--xboard" => Mode::Xboard,
            "--cnp" => Mode::Cnp,
            "--threads" => {
                settings.threads = Some(number(arg, rest.next(), MAX_THREADS)?);
                continue;
            }
            "--hash" => {
                settings.hash_mb = Some(number(arg, rest.next(), MAX_HASH_MB)?);
                continue;
            }
            // Only on their own: they end the program before it does anything else.
            "-V" | "--version" if args.len() == 1 => return Ok(Command::Version),
            "-h" | "--help" if args.len() == 1 => return Ok(Command::Help),
            "bench" => return Err("bench takes at most a network file, and no options".to_string()),
            other => return Err(format!("unknown argument '{other}'")),
        };
        if mode.replace(chosen).is_some() {
            return Err("expected at most one of --shell, --uci, --xboard and --cnp".to_string());
        }
    }
    if settings != Settings::default() && mode != Some(Mode::Cnp) {
        return Err("--threads and --hash go with --cnp".to_string());
    }
    Ok(Command::Run(mode.unwrap_or(Mode::Auto), settings))
}

/// The whole number after `flag`, from 1 to `max`.
fn number(flag: &str, value: Option<&String>, max: usize) -> Result<usize, String> {
    let value = value.ok_or_else(|| format!("{flag} needs a number"))?;
    match value.parse::<usize>() {
        Ok(n) if (1..=max).contains(&n) => Ok(n),
        _ => Err(format!(
            "{flag} must be a whole number from 1 to {max}, not '{value}'"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, String> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&owned)
    }

    #[test]
    fn no_arguments_means_auto_mode() {
        assert_eq!(
            parse_strs(&[]),
            Ok(Command::Run(Mode::Auto, Settings::default()))
        );
    }

    #[test]
    fn mode_flags() {
        let plain = Settings::default();
        assert_eq!(
            parse_strs(&["--shell"]),
            Ok(Command::Run(Mode::Shell, plain))
        );
        assert_eq!(parse_strs(&["--uci"]), Ok(Command::Run(Mode::Uci, plain)));
        assert_eq!(
            parse_strs(&["--xboard"]),
            Ok(Command::Run(Mode::Xboard, plain))
        );
        assert_eq!(parse_strs(&["--cnp"]), Ok(Command::Run(Mode::Cnp, plain)));
    }

    #[test]
    fn commands_and_info_flags() {
        assert_eq!(parse_strs(&["bench"]), Ok(Command::Bench(None)));
        assert_eq!(
            parse_strs(&["bench", "net.nnue"]),
            Ok(Command::Bench(Some("net.nnue".to_string())))
        );
        assert_eq!(parse_strs(&["-V"]), Ok(Command::Version));
        assert_eq!(parse_strs(&["--version"]), Ok(Command::Version));
        assert_eq!(parse_strs(&["-h"]), Ok(Command::Help));
        assert_eq!(parse_strs(&["--help"]), Ok(Command::Help));
    }

    #[test]
    fn rejects_unknown_and_extra_arguments() {
        assert!(parse_strs(&["--fast"]).is_err());
        assert!(parse_strs(&["--uci", "--shell"]).is_err());
        assert!(parse_strs(&["--cnp", "--cnp"]).is_err());
        assert!(parse_strs(&["--cnp", "--version"]).is_err());
        assert!(parse_strs(&["--cnp", "bench"]).is_err());
    }

    #[test]
    fn a_cnp_node_takes_threads_and_hash_in_either_order() {
        let wanted = Ok(Command::Run(
            Mode::Cnp,
            Settings {
                threads: Some(8),
                hash_mb: Some(1024),
            },
        ));
        assert_eq!(
            parse_strs(&["--cnp", "--threads", "8", "--hash", "1024"]),
            wanted
        );
        assert_eq!(
            parse_strs(&["--hash", "1024", "--threads", "8", "--cnp"]),
            wanted
        );
        assert_eq!(
            parse_strs(&["--cnp", "--threads", "2"]),
            Ok(Command::Run(
                Mode::Cnp,
                Settings {
                    threads: Some(2),
                    hash_mb: None
                }
            ))
        );
    }

    #[test]
    fn threads_and_hash_go_with_cnp_and_stay_in_range() {
        // Other modes take their settings from their own protocol.
        assert!(parse_strs(&["--threads", "4"]).is_err());
        assert!(parse_strs(&["--uci", "--hash", "64"]).is_err());
        // Whole numbers inside the engine's own limits, as UCI's options are.
        assert!(parse_strs(&["--cnp", "--threads", "0"]).is_err());
        assert!(parse_strs(&["--cnp", "--threads", "257"]).is_err());
        assert!(parse_strs(&["--cnp", "--threads", "four"]).is_err());
        assert!(parse_strs(&["--cnp", "--threads", "-1"]).is_err());
        assert!(parse_strs(&["--cnp", "--hash", "0"]).is_err());
        assert!(parse_strs(&["--cnp", "--hash", "65537"]).is_err());
        assert!(parse_strs(&["--cnp", "--threads"]).is_err());
        assert!(parse_strs(&["--cnp", "--threads", "256", "--hash", "65536"]).is_ok());
    }
}
