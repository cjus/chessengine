//! # cb2026-chess: the rules of chess
//!
//! This crate knows what a chess position is and which moves are legal in it. It knows nothing
//! about *good* moves: that's the job of `cb2026-engine`. It does no I/O and has no
//! dependencies, so everything in it can be tested exhaustively and reused by every other crate.
//!
//! ## What will live here
//!
//! The crate grows stage by stage (see `docs/PRD.md`, "Stages", and the companion book):
//!
//! - **Stage 1 (board):** squares, pieces, colours, bitboards (one 64-bit set per piece type and
//!   colour), a mailbox array for "what's on this square?" lookups, FEN parsing and printing.
//! - **Stage 2 (move generation):** attack tables for the leaping pieces, sliding attacks built
//!   the obvious way (walking rays), legal move generation with check and pin masks, castling
//!   (standard and Chess960), en passant, promotions, and *perft* to prove it all correct.
//! - **Stage 3 (magics):** magic bitboards replace the ray walks for sliding pieces.
//! - **Stage 4:** make/unmake with an undo stack, and incremental Zobrist hashing.
//!
//! ## Further reading
//!
//! - Chess Programming Wiki, [Board Representation](https://www.chessprogramming.org/Board_Representation)
//! - Chess Programming Wiki, [Bitboards](https://www.chessprogramming.org/Bitboards)
//! - Chess Programming Wiki, [Perft](https://www.chessprogramming.org/Perft)

#![forbid(unsafe_code)]
