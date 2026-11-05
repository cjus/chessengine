//! Command-line arguments.
//!
//! Kept deliberately small and dependency-free: GUIs start the engine with no arguments at all,
//! so flags are only for people at a terminal and for scripts.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use cb2026_engine::MAX_THREADS;
use cb2026_engine::tt::MAX_HASH_MB;

/// What the user asked the binary to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Run in the given mode, with the given settings.
    Run(Mode, Settings),
    /// Serve the Chess Node Protocol to clients on the network, from this address (`--serve`),
    /// with the given settings and network options. Needs a build with the `net` feature.
    Serve(SocketAddr, Settings, Network),
    /// Open a window in which one new client may pair with this node (`--pair`), keeping its
    /// files in the given directory, or the default one. Needs a build with the `net` feature.
    Pair(Option<PathBuf>),
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

/// How `--serve` is protected, beyond where it listens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Network {
    /// Use TLS and approved clients even on a loopback address. Any other address uses them
    /// whether or not this is set, because nothing else is safe to leave open.
    pub tls: bool,
    /// Where the node's identity, approved clients and pairing window are kept, instead of the
    /// default directory of the system.
    pub data_dir: Option<PathBuf>,
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
  --serve [ADDR] Serve the Chess Node Protocol over the network, one client at a time. ADDR is
                 ADDRESS:PORT or just a port (default 127.0.0.1:4026). On this computer
                 only (a loopback address) it is plain; on any other address, such as
                 0.0.0.0:4026, it is encrypted and only clients you approved may connect.
                 Builds with --features net
  --tls          With --serve: encrypt and require approved clients on a loopback address too
  --pair         Approve a new client: shows this node's fingerprint and a one-time code to
                 give the client within five minutes. Run it while --serve is running
  --data-dir DIR With --serve or --pair: keep the node's certificate, approved clients and
                 pairing code in DIR (default: cb2026 in your user data directory)
  --threads N    With --cnp or --serve: search with N threads (1-256, default 1)
  --hash MB      With --cnp or --serve: a transposition table of MB megabytes (1-65536,
                 default 16)
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
    let mut serve = None;
    let (mut tls, mut pair, mut data_dir) = (false, false, None);
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
            "--serve" => {
                // An address may follow; anything starting with `-` is the next option.
                let address = match rest.as_slice().first() {
                    Some(next) if !next.starts_with('-') => address(rest.next().unwrap())?,
                    _ => default_address(),
                };
                if serve.replace(address).is_some() {
                    return Err("--serve is given once".to_string());
                }
                continue;
            }
            "--tls" => {
                tls = true;
                continue;
            }
            "--pair" => {
                pair = true;
                continue;
            }
            "--data-dir" => {
                let dir = rest.next().filter(|d| !d.starts_with('-'));
                let dir = dir.ok_or_else(|| "--data-dir needs a directory".to_string())?;
                if data_dir.replace(PathBuf::from(dir)).is_some() {
                    return Err("--data-dir is given once".to_string());
                }
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
    if pair {
        if serve.is_some() || tls || mode.is_some() || settings != Settings::default() {
            return Err("--pair stands alone, with --data-dir if the files are elsewhere".into());
        }
        return Ok(Command::Pair(data_dir));
    }
    if let Some(address) = serve {
        if mode.is_some() {
            return Err("--serve can't be combined with --shell, --uci, --xboard or --cnp".into());
        }
        return Ok(Command::Serve(address, settings, Network { tls, data_dir }));
    }
    if tls || data_dir.is_some() {
        return Err("--tls and --data-dir go with --serve (--data-dir also with --pair)".into());
    }
    if settings != Settings::default() && mode != Some(Mode::Cnp) {
        return Err("--threads and --hash go with --cnp or --serve".to_string());
    }
    Ok(Command::Run(mode.unwrap_or(Mode::Auto), settings))
}

/// The address `--serve` listens on when none is given: this computer only.
fn default_address() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT)
}

/// An `ADDRESS:PORT` (`127.0.0.1:4026`, `[::1]:4026`), or just a port on this computer.
fn address(text: &str) -> Result<SocketAddr, String> {
    if let Ok(port) = text.parse::<u16>() {
        return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    text.parse().map_err(|_| {
        format!("--serve takes ADDRESS:PORT (such as 127.0.0.1:4026) or a port, not '{text}'")
    })
}

/// The port `--serve` uses when none is given (provisional; see `cb2026_net::DEFAULT_PORT`).
const DEFAULT_PORT: u16 = 4026;

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
    fn serve_takes_an_address_a_port_or_nothing() {
        let plain = Settings::default();
        let local = |port| SocketAddr::from(([127, 0, 0, 1], port));
        assert_eq!(
            parse_strs(&["--serve"]),
            Ok(Command::Serve(local(4026), plain, Network::default()))
        );
        assert_eq!(
            parse_strs(&["--serve", "5000"]),
            Ok(Command::Serve(local(5000), plain, Network::default()))
        );
        assert_eq!(
            parse_strs(&["--serve", "127.0.0.1:0"]),
            Ok(Command::Serve(local(0), plain, Network::default()))
        );
        // The address is parsed, not judged: the listener refuses what it won't serve.
        assert_eq!(
            parse_strs(&["--serve", "[::1]:4026"]),
            Ok(Command::Serve(
                SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 4026)),
                plain,
                Network::default()
            ))
        );
        assert_eq!(
            parse_strs(&["--serve", "--threads", "4"]),
            Ok(Command::Serve(
                local(4026),
                Settings {
                    threads: Some(4),
                    hash_mb: None
                },
                Network::default()
            ))
        );
        assert_eq!(
            parse_strs(&["--threads", "4", "--serve", "6000", "--hash", "64"]),
            Ok(Command::Serve(
                local(6000),
                Settings {
                    threads: Some(4),
                    hash_mb: Some(64)
                },
                Network::default()
            ))
        );
    }

    #[test]
    fn serve_is_refused_with_a_mode_twice_or_with_a_bad_address() {
        assert!(parse_strs(&["--serve", "--cnp"]).is_err());
        assert!(parse_strs(&["--uci", "--serve"]).is_err());
        assert!(parse_strs(&["--serve", "--serve"]).is_err());
        assert!(parse_strs(&["--serve", "localhost"]).is_err());
        assert!(parse_strs(&["--serve", "70000"]).is_err());
        assert!(parse_strs(&["--serve", "1.2.3.4"]).is_err());
    }

    #[test]
    fn serve_takes_tls_and_a_data_directory_and_pair_stands_alone() {
        let plain = Settings::default();
        let network = Network {
            tls: true,
            data_dir: Some(PathBuf::from("/tmp/cb")),
        };
        assert_eq!(
            parse_strs(&["--serve", "0.0.0.0:4026", "--tls", "--data-dir", "/tmp/cb"]),
            Ok(Command::Serve(
                SocketAddr::from(([0, 0, 0, 0], 4026)),
                plain,
                network
            ))
        );
        assert_eq!(parse_strs(&["--pair"]), Ok(Command::Pair(None)));
        assert_eq!(
            parse_strs(&["--data-dir", "/tmp/cb", "--pair"]),
            Ok(Command::Pair(Some(PathBuf::from("/tmp/cb"))))
        );
        // Each goes only where it means something.
        for bad in [
            &["--tls"][..],
            &["--cnp", "--tls"][..],
            &["--data-dir", "/tmp/cb"][..],
            &["--data-dir"][..],
            &["--data-dir", "--serve"][..],
            &["--serve", "--data-dir", "a", "--data-dir", "b"][..],
            &["--pair", "--serve"][..],
            &["--pair", "--tls"][..],
            &["--pair", "--cnp"][..],
            &["--pair", "--threads", "2"][..],
        ] {
            assert!(parse_strs(bad).is_err(), "{bad:?}");
        }
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
