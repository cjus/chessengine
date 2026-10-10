//! Command-line arguments.
//!
//! Kept deliberately small and dependency-free: GUIs start the engine with no arguments at all,
//! so flags are only for people at a terminal and for scripts.

/// What the user asked the binary to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Run in the given mode.
    Run(Mode),
    /// Run the fixed benchmark and print its node count and speed.
    Bench,
    /// Print the engine name and version.
    Version,
    /// Print usage.
    Help,
}

/// How the binary talks to whoever started it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Decide from the input: a terminal gets the shell; otherwise the first line decides
    /// between UCI, XBoard and shell commands (see [`cb2026_protocol::detect`]).
    Auto,
    /// The interactive shell.
    Shell,
    /// The UCI protocol.
    Uci,
    /// The XBoard / CECP protocol.
    Xboard,
}

/// Usage text for `--help` and argument errors.
pub const USAGE: &str = "\
Usage: cb2026 [COMMAND | OPTION]

With no arguments, cb2026 opens the shell in a terminal, or speaks UCI or XBoard when a GUI
starts it (the GUI's first line, `uci` or `xboard`, decides).

Commands:
  bench          Run the fixed benchmark; prints total nodes and speed

Options:
  --shell        Force the interactive shell
  --uci          Force UCI mode
  --xboard       Force XBoard/CECP mode
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
            "bench" => Ok(Command::Bench),
            "-V" | "--version" => Ok(Command::Version),
            "-h" | "--help" => Ok(Command::Help),
            other => Err(format!("unknown argument '{other}'")),
        },
        _ => Err("expected at most one argument".to_string()),
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
    }

    #[test]
    fn commands_and_info_flags() {
        assert_eq!(parse_strs(&["bench"]), Ok(Command::Bench));
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
