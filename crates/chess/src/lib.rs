//! # cb2026-chess: the rules of chess
//!
//! This crate knows what a chess position is and which moves are legal in it. It knows nothing
//! about *good* moves: that's the job of `cb2026-engine`. It does no I/O and has no
//! dependencies, so everything in it can be tested exhaustively and reused by every other crate.
//!
//! ## What's here
//!
//! - [`square`]: squares, files and ranks, and how the 64 squares are numbered.
//! - [`piece`]: colours, piece types and pieces.
//! - [`bitboard`]: sets of squares packed into a `u64`, the core data structure.
//! - [`castling`]: castling rights, stored so that Chess960 works the same way.
//! - [`position`]: a full position, kept as bitboards and as a mailbox array.
//! - [`fen`]: reading and writing positions in Forsyth–Edwards Notation.
//! - [`attacks`]: which squares each piece attacks, and the lines between squares.
//! - [`magic`]: magic bitboards, fast table lookups for sliding-piece attacks.
//! - [`moves`]: moves packed into 16 bits, and move lists.
//! - [`movegen`]: legal move generation with check and pin masks.
//! - [`makemove`]: playing a move on a position.
//! - [`perft`]: counting positions to prove move generation correct.
//!
//! ## What's coming
//!
//! The crate grows stage by stage (see `docs/PRD.md`, "Stages", and the companion book):
//!
//! - **Stage 4:** make/unmake with an undo stack, and incremental Zobrist hashing.
//!
//! ## Further reading
//!
//! - Chess Programming Wiki, [Board Representation](https://www.chessprogramming.org/Board_Representation)
//! - Chess Programming Wiki, [Bitboards](https://www.chessprogramming.org/Bitboards)
//! - Chess Programming Wiki, [Perft](https://www.chessprogramming.org/Perft)

#![forbid(unsafe_code)]

pub mod attacks;
pub mod bitboard;
pub mod castling;
pub mod fen;
pub mod magic;
pub mod makemove;
pub mod movegen;
pub mod moves;
pub mod perft;
pub mod piece;
pub mod position;
pub mod square;

pub use bitboard::Bitboard;
pub use castling::{CastlingRights, CastlingSide};
pub use fen::{FenError, STARTING_FEN};
pub use moves::{Move, MoveKind, MoveList};
pub use piece::{Color, Piece, PieceType};
pub use position::Position;
pub use square::{File, Rank, Square};
