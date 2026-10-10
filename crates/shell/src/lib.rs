//! # cb2026-shell: the interactive shell
//!
//! Running `cb2026` in a terminal opens a command shell for playing and studying chess with the
//! engine. It is a front end like a GUI, but it can also show the engine's internals.
//!
//! ## What will live here
//!
//! - **From stage 1:** board display in Unicode or ASCII, with colour when the terminal supports
//!   it (and `NO_COLOR` respected).
//! - **From stages 2–3:** `perft`, `divide`, and views of bitboards, attack maps and magic
//!   lookups.
//! - **From stage 4:** playing complete games against the engine with clocks and levels, and a
//!   live display of the engine's thinking.
//! - **From stage 9:** the explain commands: `eval` (term-by-term evaluation), `explain`,
//!   `tree` and `stats`.
//!
//! The full command list is in `docs/PRD.md`, "The interactive shell".

#![forbid(unsafe_code)]
