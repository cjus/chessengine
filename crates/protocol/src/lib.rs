//! # cb2026-protocol: talking to chess GUIs
//!
//! Chess GUIs don't link against an engine; they start it as a separate program and talk to it
//! over standard input and output using a text protocol. Two protocols matter:
//!
//! - **UCI** (Universal Chess Interface) is what almost every modern GUI uses. The GUI owns the
//!   game state and sends the engine a position and a "go" command each move.
//! - **XBoard / CECP** (Chess Engine Communication Protocol) is older and used by XBoard and some
//!   other GUIs. Here the engine keeps track of the game itself.
//!
//! This crate turns those text commands into calls on the Engine API and turns search results
//! back into protocol output. [`uci`] speaks UCI and [`xboard`] speaks
//! XBoard.
//!
//! ## Further reading
//!
//! - Chess Programming Wiki, [UCI](https://www.chessprogramming.org/UCI)
//! - Chess Programming Wiki, [Chess Engine Communication Protocol](https://www.chessprogramming.org/Chess_Engine_Communication_Protocol)

#![forbid(unsafe_code)]

pub mod uci;
pub mod xboard;

/// A text protocol a GUI can use to talk to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// The Universal Chess Interface.
    Uci,
    /// The XBoard / CECP protocol, version 2.
    Xboard,
}

/// Works out which protocol a GUI is speaking from the first line it sends.
///
/// A UCI GUI always starts with `uci` and an XBoard GUI with `xboard`, so one line is enough to
/// tell them apart. That lets a single `cb2026` binary serve both kinds of GUI with no
/// command-line flags. Surrounding whitespace is ignored, which also strips the `\r` that
/// Windows GUIs send before `\n`.
///
/// Returns `None` for anything else, which the binary treats as shell input.
///
/// ```
/// use cb2026_protocol::{detect, Protocol};
///
/// assert_eq!(detect("uci\r\n"), Some(Protocol::Uci));
/// assert_eq!(detect("xboard"), Some(Protocol::Xboard));
/// assert_eq!(detect("new"), None);
/// ```
pub fn detect(first_line: &str) -> Option<Protocol> {
    match first_line.trim() {
        "uci" => Some(Protocol::Uci),
        "xboard" => Some(Protocol::Xboard),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_uci_and_xboard() {
        assert_eq!(detect("uci"), Some(Protocol::Uci));
        assert_eq!(detect("xboard"), Some(Protocol::Xboard));
    }

    #[test]
    fn ignores_surrounding_whitespace_and_windows_line_endings() {
        assert_eq!(detect("  uci \r\n"), Some(Protocol::Uci));
        assert_eq!(detect("xboard\r\n"), Some(Protocol::Xboard));
    }

    #[test]
    fn rejects_other_commands() {
        // Commands are case-sensitive, and a longer command that merely starts with "uci" is
        // not the UCI handshake.
        for line in [
            "",
            "UCI",
            "ucinewgame",
            "uci extra",
            "isready",
            "e4",
            "help",
        ] {
            assert_eq!(detect(line), None, "line {line:?}");
        }
    }
}
