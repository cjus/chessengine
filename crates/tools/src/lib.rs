//! # cb2026-tools: developer tools
//!
//! Tools that help build the engine but are not part of what ships to players:
//!
//! - **Texel tuner (stage 16):** fits the evaluation weights to the results of self-play games.
//! - **Self-play data generation (stage 16):** plays games against itself and records quiet
//!   positions with their results, for tuning (and later for NNUE training).
//! - **Test helpers:** shared code for perft suites, EPD suites and the protocol conformance
//!   tests.
//!
//! ## What's here now
//!
//! - [`uci_client`], [`game`] and [`match_runner`]: a small tournament manager that plays two
//!   UCI engines against each other, used by the `match` tool to measure changes.
//! - [`sprt`]: Elo estimates and the sequential probability ratio test.
//! - [`gauntlet`]: a rating on an outside scale (like CCRL Blitz) from matches against
//!   reference engines, used by the `gauntlet` tool.
//! - [`selfplay`]: self-play data generation for tuning (the `datagen` tool).
//! - [`texel`]: the Texel tuner's error, gradient and optimiser (the `tune` tool).
//! - [`mistakes`]: how often a strength level makes inaccuracies, mistakes and blunders, as
//!   Lichess counts them, to compare the levels with people (the `mistakes` tool).
//! - The `perft-suite`, `find-magics`, `match`, `gauntlet`, `datagen`, `tune` and `mistakes`
//!   command-line tools (`src/bin/`).

#![forbid(unsafe_code)]

pub mod game;
pub mod gauntlet;
pub mod match_runner;
pub mod mistakes;
pub mod selfplay;
pub mod sprt;
pub mod texel;
pub mod uci_client;
