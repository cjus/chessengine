//! # cb2026-trainer: learning the network's weights
//!
//! The trainer turns self-play records (see `cb2026_nnue::record`) into a network (see
//! `cb2026_nnue`). It is written for reading: no machine-learning library, and every gradient is
//! worked out by hand in [`backprop`], so the book can follow each step.
//!
//! - [`example`]: a record turned into what the network trains on, its inputs from both views
//!   and its **target**, a blend of the search's score and the game's result.
//! - [`backprop`]: the forward pass, the loss and the backward pass: the gradient of the loss
//!   with respect to every weight.
//! - [`adam`]: AdamW, the optimiser that turns gradients into weight changes.
//! - [`scale`]: fitting the eval scale, the link between centipawns and expected results.
//! - [`data`]: the records in memory, split into training and validation sets.
//! - [`train`]: the training loop, on all the CPU's cores.
//! - [`checkpoint`]: saving training to resume it later.
//!
//! This is the CPU version: it is the reference, and the version the book explains. A GPU
//! version, computing the same thing faster, will be tested against it.
//!
//! ## How training works
//!
//! 1. Start from small random weights.
//! 2. Take a **batch** of positions. For each, compute the network's output (the forward pass),
//!    compare `σ(output)` with the target (the loss), and work out how much each weight
//!    contributed to the error (the backward pass).
//! 3. Average those gradients over the batch and move every weight a little against its
//!    gradient (AdamW), then clip it to the limit that keeps the integer network safe.
//! 4. Repeat over the whole training set (an **epoch**), many times, lowering the step size
//!    towards the end. After each epoch, measure the loss on positions held back (the
//!    **validation** set): if it stops falling while the training loss keeps falling, the
//!    network is learning the training positions by heart rather than chess.
//!
//! ## Pitfalls
//!
//! - **The target's side.** Records keep scores and results from White's side; the network
//!   scores for the side to move. [`example`] turns both around for Black.
//! - **The eval scale** must match how cb2026's centipawns relate to results (see [`scale`]).
//! - **Weight clipping** after every step is what makes the quantised network safe (see
//!   `cb2026_nnue::float::WEIGHT_LIMIT`).
//! - **Overfitting:** watch the validation loss.
//!
//! ## Further reading
//!
//! - Chess Programming Wiki, [NNUE](https://www.chessprogramming.org/NNUE).
//! - Diederik Kingma and Jimmy Ba, "Adam: A Method for Stochastic Optimization" (2015); Ilya
//!   Loshchilov and Frank Hutter, "Decoupled Weight Decay Regularization" (2019).

#![forbid(unsafe_code)]

pub mod adam;
pub mod backprop;
pub mod checkpoint;
pub mod data;
pub mod example;
pub mod scale;
pub mod train;

#[cfg(test)]
pub(crate) mod test_support;
