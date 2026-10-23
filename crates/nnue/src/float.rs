//! The network with floating-point weights, as the trainer learns it.
//!
//! The trainer works in `f32`: gradients need fractions. When training ends, [`FloatNetwork::quantize`]
//! turns the weights into the whole numbers the engine runs ([`Network`]). This module is also
//! the reference the integer network is tested against: the two give nearly the same scores,
//! differing only by rounding.
//!
//! ## The forward pass
//!
//! For each view, the hidden layer is the biases plus the weights of every input that is on.
//! Each hidden value goes through the activation, `clamp(x, 0, 1)` or its square, and the output
//! is the sum of those values times the output weights (the side to move's half first), plus the
//! output bias. Times the eval scale, that is centipawns.
//!
//! ## The weight limit
//!
//! Every weight and first-layer bias must stay within ±[`WEIGHT_LIMIT`] = ±1.98. The trainer
//! clips them after each step, and [`FloatNetwork::quantize`] clips again, so the integer
//! network's sums can't overflow (see [`network`](crate::network), "Why it can't overflow"). The
//! output bias isn't limited: it is stored in an `i32`.

use cb2026_chess::{Color, Position};

use crate::features::{self, INPUTS};
use crate::network::{Activation, NetError, Network, QA, QB};

// ANCHOR: weight_limit
/// The largest a weight or first-layer bias may be, either way. 1.98 × 64 rounds to 127, the
/// largest output weight that keeps an activation times its weight inside an `i16`.
pub const WEIGHT_LIMIT: f32 = 1.98;
// ANCHOR_END: weight_limit

/// A network with floating-point weights. The fields are public: the trainer reads and changes
/// them directly.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatNetwork {
    /// The hidden layer's size, per view: a multiple of 32, at most 4096.
    pub hidden: usize,
    /// How a hidden value becomes the output layer's input.
    pub activation: Activation,
    /// Centipawns per unit of output.
    pub eval_scale: u32,
    /// [`INPUTS`] rows of `hidden` weights, one row per input.
    pub input_weights: Vec<f32>,
    /// One bias per hidden value.
    pub input_biases: Vec<f32>,
    /// `2 × hidden` weights: the side to move's half, then the other side's.
    pub output_weights: Vec<f32>,
    /// The output's bias.
    pub output_bias: f32,
}

impl FloatNetwork {
    /// A network of the given shape with every weight zero, for the trainer to fill in.
    pub fn zeros(hidden: usize, activation: Activation, eval_scale: u32) -> FloatNetwork {
        FloatNetwork {
            hidden,
            activation,
            eval_scale,
            input_weights: vec![0.0; INPUTS * hidden],
            input_biases: vec![0.0; hidden],
            output_weights: vec![0.0; 2 * hidden],
            output_bias: 0.0,
        }
    }

    /// The hidden layer for `pos` seen from `view`'s side, before the activation.
    pub fn hidden_layer(&self, pos: &Position, view: Color) -> Vec<f32> {
        let mut values = self.input_biases.clone();
        for input in features::active(pos, view) {
            let row = &self.input_weights[input * self.hidden..(input + 1) * self.hidden];
            for (value, weight) in values.iter_mut().zip(row) {
                *value += weight;
            }
        }
        values
    }

    /// The activation of one hidden value.
    pub fn activate(&self, value: f32) -> f32 {
        let clipped = value.clamp(0.0, 1.0);
        match self.activation {
            Activation::ClippedRelu => clipped,
            Activation::SquaredClippedRelu => clipped * clipped,
        }
    }

    // ANCHOR: output
    /// The network's output for `pos`, from the side to move's view, before the eval scale.
    /// The trainer compares `σ(output)` with its targets.
    pub fn output(&self, pos: &Position) -> f32 {
        let us = pos.side_to_move();
        let (our_weights, their_weights) = self.output_weights.split_at(self.hidden);
        let half = |view: Color, weights: &[f32]| -> f32 {
            self.hidden_layer(pos, view)
                .iter()
                .zip(weights)
                .map(|(&value, weight)| self.activate(value) * weight)
                .sum()
        };
        half(us, our_weights) + half(!us, their_weights) + self.output_bias
    }
    // ANCHOR_END: output

    /// The position's value in centipawns, from the side to move's view.
    pub fn evaluate(&self, pos: &Position) -> f32 {
        self.output(pos) * self.eval_scale as f32
    }

    // ANCHOR: quantize
    /// The network with whole-number weights: each weight clipped to ±[`WEIGHT_LIMIT`], times
    /// its scale, rounded.
    ///
    /// # Panics
    ///
    /// If a vector's length doesn't match `hidden`.
    pub fn quantize(&self) -> Result<Network, NetError> {
        let scaled = |values: &[f32], scale: i32| -> Vec<i16> {
            values
                .iter()
                .map(|w| (w.clamp(-WEIGHT_LIMIT, WEIGHT_LIMIT) * scale as f32).round() as i16)
                .collect()
        };
        Network::from_parts(
            self.hidden,
            self.activation,
            self.eval_scale,
            scaled(&self.input_weights, QA),
            scaled(&self.input_biases, QA),
            scaled(&self.output_weights, QB),
            (self.output_bias * (QA * QB) as f32).round() as i32,
        )
    }
    // ANCHOR_END: quantize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::{DEFAULT_HIDDEN, MAX_INPUT_WEIGHT, MAX_OUTPUT_WEIGHT};
    use crate::test_support::{mirror, random_float_network, random_game_positions};

    #[test]
    fn quantizing_clips_to_the_weight_limit() {
        let mut float = FloatNetwork::zeros(32, Activation::ClippedRelu, 146);
        float.input_weights[0] = 5.0;
        float.input_weights[1] = -5.0;
        float.input_weights[2] = 0.5;
        float.output_weights[0] = 5.0;
        float.output_bias = 0.25;
        let net = float.quantize().unwrap();
        // Back to floats and up to each scale again, to see the stored whole numbers.
        let back = net.to_float();
        let stored = |value: f32, scale: i32| (value * scale as f32).round();
        assert_eq!(
            stored(back.input_weights[0], QA),
            f32::from(MAX_INPUT_WEIGHT)
        );
        assert_eq!(
            stored(back.input_weights[1], QA),
            -f32::from(MAX_INPUT_WEIGHT)
        );
        assert_eq!(stored(back.input_weights[2], QA), 128.0); // 0.5 × 255 = 127.5, rounded
        assert_eq!(
            stored(back.output_weights[0], QB),
            f32::from(MAX_OUTPUT_WEIGHT)
        );
        assert_eq!(stored(back.output_bias, QA * QB), 4080.0); // 0.25 × 16,320
    }

    /// The mean and largest difference, in centipawns, between two ways of scoring positions.
    fn differences(
        positions: &[Position],
        a: impl Fn(&Position) -> f32,
        b: impl Fn(&Position) -> f32,
    ) -> (f32, f32) {
        let errors: Vec<f32> = positions.iter().map(|p| (a(p) - b(p)).abs()).collect();
        let worst = errors.iter().copied().fold(0.0, f32::max);
        (errors.iter().sum::<f32>() / errors.len() as f32, worst)
    }

    #[test]
    fn the_integer_network_computes_what_the_float_one_does() {
        // Against the float network with the integer one's own weights, only the integer
        // divisions differ: under a centipawn for the last one, plus at most 146 / 16,320 of a
        // centipawn for SCReLU's division by 255. (Measured 2026-10-23: 0.9996 and 1.0071.)
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            let net = random_float_network(DEFAULT_HIDDEN, activation, 21)
                .quantize()
                .unwrap();
            let same = net.to_float();
            let positions = random_game_positions(22, 20, 120);
            let (_, worst) = differences(
                &positions,
                |p| net.evaluate_position(p) as f32,
                |p| same.evaluate(p),
            );
            assert!(worst < 1.05, "{activation:?}: worst {worst}");
        }
    }

    #[test]
    fn rounding_the_weights_costs_a_few_centipawns() {
        // Each first-layer weight moves by up to 1/510 and each output weight by up to 1/128.
        // Over 33 inputs and 512 hidden values those add up to a few centipawns at this scale,
        // more for these random weights than for trained ones, which are mostly smaller.
        // (Measured 2026-10-23: means 3.9 and 2.3, worst 17.5 and 11.6.)
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            let float = random_float_network(DEFAULT_HIDDEN, activation, 21);
            let net = float.quantize().unwrap();
            let positions = random_game_positions(22, 20, 120);
            let (mean, worst) = differences(
                &positions,
                |p| net.evaluate_position(p) as f32,
                |p| float.evaluate(p),
            );
            assert!(
                mean < 8.0 && worst < 30.0,
                "{activation:?}: mean {mean}, worst {worst}"
            );
        }
    }

    #[test]
    fn mirrored_positions_score_the_same_in_floats_too() {
        let float = random_float_network(DEFAULT_HIDDEN, Activation::SquaredClippedRelu, 23);
        for pos in random_game_positions(24, 10, 120) {
            // The inputs are added in a different order, so the sums may differ in the last bit.
            let (a, b) = (float.evaluate(&pos), float.evaluate(&mirror(&pos)));
            assert!((a - b).abs() < 0.01, "{a} vs {b} in {}", pos.to_fen());
        }
    }
}
