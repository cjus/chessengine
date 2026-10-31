//! Command-line arguments.
//!
//! Kept deliberately small and dependency-free: GUIs start the engine with no arguments at all,
//! so flags are only for people at a terminal and for scripts.

/// What the user asked the binary to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Run in the given mode.
    Run(Mode),
    /// Run the fixed benchmark and print its node count and speed, evaluating with the NNUE
    /// network in this file, or with none (`none`, the hand-crafted evaluation), if given; with
    /// the built-in network otherwise.
    Bench(Option<String>),
    /// Print the engine name and version, with the commit it was built from.
    Version,
    /// Print usage.
    Help,
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
  -V, --version  Print name and version
  -h, --help     Print this help";

/// Parses the arguments after the program name.
///
/// At most one command or option is accepted; anything else is an error with a message for the
/// user.
pub fn parse(args: &[String]) -> Result<Command, String> {
    match args {
        [] => Ok(Command::Run(Mode::Auto)),
        [arg] => match arg.as_str() {
            "--shell" => Ok(Command::Run(Mode::Shell)),
            "--uci" => Ok(Command::Run(Mode::Uci)),
            "--xboard" => Ok(Command::Run(Mode::Xboard)),
            "--cnp" => Ok(Command::Run(Mode::Cnp)),
            "bench" => Ok(Command::Bench(None)),
            "-V" | "--version" => Ok(Command::Version),
            "-h" | "--help" => Ok(Command::Help),
            other => Err(format!("unknown argument '{other}'")),
        },
        [command, net] if command == "bench" => Ok(Command::Bench(Some(net.clone()))),
        _ => Err("expected at most one argument (or bench and a network file)".to_string()),
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
        assert_eq!(parse_strs(&[]), Ok(Command::Run(Mode::Auto)));
    }

    #[test]
    fn mode_flags() {
        assert_eq!(parse_strs(&["--shell"]), Ok(Command::Run(Mode::Shell)));
        assert_eq!(parse_strs(&["--uci"]), Ok(Command::Run(Mode::Uci)));
        assert_eq!(parse_strs(&["--xboard"]), Ok(Command::Run(Mode::Xboard)));
        assert_eq!(parse_strs(&["--cnp"]), Ok(Command::Run(Mode::Cnp)));
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
    }
}
