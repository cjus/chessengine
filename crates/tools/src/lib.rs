//! # cb2026-tools: developer tools
//!
//! Tools that help build the engine but are not part of what ships to players:
//!
//! - **Texel tuner (stage 16):** fits the evaluation weights to the results of self-play games.
//! - **Self-play data generation (stage 16):** plays games against itself and records quiet
//!   positions with their results, for tuning (and later for NNUE training).
//! - **Test helpers:** shared code for perft suites, EPD suites and the protocol conformance
//!   tests.

#![forbid(unsafe_code)]
