//! # cb2026-nnue: the neural network evaluation's definition
//!
//! An **NNUE** ("efficiently updatable neural network") evaluates a position with a small neural
//! network instead of hand-written terms. This crate says exactly what cb2026's network is, so
//! that the trainer, which learns its weights, and the engine, which uses them, can't disagree:
//!
//! - [`features`]: the network's 768 inputs, which piece stands on which square, and which
//!   inputs a move switches on and off.
//! - [`network`]: the network with whole-number weights that the engine runs, its
//!   [`Accumulator`] (the first layer, updated move by move), and its file format.
//! - [`float`]: the same network with floating-point weights, as the trainer learns it, and the
//!   conversion to whole numbers.
//!
//! It does no I/O: networks are read from and written to byte slices.
//!
//! ## The network
//!
//! ```text
//!   768 inputs ──► 256 hidden ─┐                 (from the side to move's view)
//!                              ├──► 1 output ──► centipawns
//!   768 inputs ──► 256 hidden ─┘                 (from the other side's view)
//! ```
//!
//! Both halves share one set of first-layer weights; the second half sees the board flipped,
//! with the colours named "ours" and "theirs" (see [`features`]). Each hidden value is clipped to
//! 0..1 and squared, the **SCReLU** activation (plain clipping, **CReLU**, is also supported), and
//! the output layer adds them up with its own weights. The hidden layer's size is read from the
//! file, so nets of other sizes load too.
//!
//! ## Why it works, and why it is fast
//!
//! A network can learn what a hand-crafted evaluation can't express, from millions of positions
//! of cb2026's own games, each labelled with its search score and game result. It stays fast
//! because a move changes at most four of the 768 inputs: rather than recomputing the first
//! layer, the engine adds and subtracts a few rows of weights (the **accumulator**). Whole-number
//! weights make that arithmetic exact and quick (see [`network`]).
//!
//! ## Pitfalls
//!
//! - **Trainer and engine must agree** on the inputs' numbering, the layers and the integer
//!   scales. That's why they live here, used by both, with tests that pin them down.
//! - **The output's scale must match the hand-crafted evaluation's**, or every pruning margin
//!   tuned in centipawns silently changes meaning. See [`network`], "The output".
//! - **Overflow:** the integers are sized so that no legal position can overflow them, and the
//!   training clips the weights to make sure (see [`float::WEIGHT_LIMIT`]).
//!
//! ## Credits and further reading
//!
//! This shape of network, a "(768 → N) × 2 → 1" perspective network with SCReLU, integer scales
//! of 255 and 64 and weights clipped to ±1.98, is the common first network of open-source engines
//! trained with bullet (MIT); the ideas are used here, not its code.
//!
//! - Chess Programming Wiki, [NNUE](https://www.chessprogramming.org/NNUE)
//! - Yu Nasu, *Efficiently Updatable Neural-Network-based Evaluation Functions for Computer
//!   Shogi* (2018), which introduced NNUE.

#![forbid(unsafe_code)]

pub mod features;
pub mod float;
pub mod network;

pub use features::{Changes, INPUTS};
pub use float::FloatNetwork;
pub use network::{Accumulator, Activation, NetError, Network};

#[cfg(test)]
pub(crate) mod test_support;
