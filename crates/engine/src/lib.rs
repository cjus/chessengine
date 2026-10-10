//! # cb2026-engine: choosing a move
//!
//! Given a position from `cb2026-chess`, this crate decides what to play: it searches the game
//! tree and evaluates the positions it reaches. It does no I/O; the protocols and the shell
//! drive it through the Engine API and receive search progress through a callback.
//!
//! ## What will live here
//!
//! - **Search (stages 5–14):** negamax alpha-beta, iterative deepening, quiescence search, a
//!   transposition table, move ordering, principal variation search, and the pruning and
//!   reduction techniques modern engines use. Each technique can be switched off at runtime so
//!   its value can be measured.
//! - **Evaluation (stages 9, 15–16):** a tapered, hand-crafted evaluation whose weights are tuned
//!   on self-play games, with a per-term trace for the shell's `eval` command.
//! - **Everything else a real engine needs (stages 6, 17–19):** time management, Lazy SMP,
//!   Syzygy tablebases, a Polyglot opening book, and strength limiting.
//!
//! ## `unsafe` policy
//!
//! This crate denies `unsafe` code rather than forbidding it, because two later modules may need
//! it: SIMD evaluation and the lock-free transposition table. Each such module will opt in with
//! `#[allow(unsafe_code)]` and justify every `unsafe` block with a `// SAFETY:` comment.

#![deny(unsafe_code)]

/// The engine's name, as reported to GUIs (`id name` in UCI, `myname` in XBoard).
pub const NAME: &str = "cb2026";

/// The engine's version, taken from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The full engine identification, for example `cb2026 0.1.0`.
///
/// GUIs show this string in their engine lists, so it includes the version: testers need to
/// know exactly which build played a game.
pub fn id_name() -> String {
    format!("{NAME} {VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_name_is_name_then_version() {
        assert_eq!(id_name(), format!("cb2026 {VERSION}"));
    }
}
