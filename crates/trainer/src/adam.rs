//! AdamW: turning gradients into weight changes.
//!
//! Plain gradient descent moves each weight by `learning rate × gradient`. That works badly
//! here, because gradients differ hugely in size: an input like "our king on g1" is on in a large
//! share of positions, one like "their queen on a1" in very few. **Adam** keeps, for each
//! weight, running averages of its gradient (the first moment, `m`) and of the gradient's square
//! (the second moment, `v`), and moves the weight by about `m / √v`. A weight whose gradient
//! points the same way step after step moves by about the learning rate, whatever the gradient's
//! size; one whose gradient keeps changing sign hardly moves. The averages start at zero, so
//! early on they are divided by `1 − β^t` to correct the bias.
//!
//! **Weight decay** pulls each weight a little towards zero at every step, so weights that no
//! data supports fade away instead of drifting. AdamW applies it to the weights directly rather
//! than through the gradient, which works better with Adam.
//!
//! After every step the first layer's weights and biases and the output weights are clipped to
//! ±[`WEIGHT_LIMIT`], the bound that keeps the integer network from overflowing. The output bias
//! is left alone: it is stored in an `i32`.
//!
//! Chapter 16's Texel tuner uses the same Adam. See Kingma and Ba, "Adam: A Method for
//! Stochastic Optimization" (2015), and Loshchilov and Hutter, "Decoupled Weight Decay
//! Regularization" (2019).

use cb2026_nnue::FloatNetwork;
use cb2026_nnue::float::WEIGHT_LIMIT;

use crate::backprop::Gradients;

/// The AdamW optimiser's settings and state.
#[derive(Clone, Debug, PartialEq)]
pub struct AdamW {
    /// How far a step moves a weight, roughly.
    pub learning_rate: f32,
    /// The share of each weight taken away at every step, times the learning rate.
    pub weight_decay: f32,
    /// Steps taken so far.
    pub(crate) steps: u64,
    /// The running average of each weight's gradient.
    pub(crate) first: Gradients,
    /// The running average of each weight's squared gradient.
    pub(crate) second: Gradients,
}

impl AdamW {
    /// How fast the first moment forgets (Kingma and Ba's suggestion).
    pub const BETA1: f32 = 0.9;
    /// How fast the second moment forgets.
    pub const BETA2: f32 = 0.999;
    /// Keeps the step finite when a gradient has always been zero.
    pub const EPSILON: f32 = 1e-8;

    /// A fresh optimiser for a network with a hidden layer of `hidden`.
    pub fn new(hidden: usize, learning_rate: f32, weight_decay: f32) -> AdamW {
        AdamW {
            learning_rate,
            weight_decay,
            steps: 0,
            first: Gradients::zeros(hidden),
            second: Gradients::zeros(hidden),
        }
    }

    /// How many steps it has taken.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    // ANCHOR: step
    /// Moves every weight of `net` one step against `grads`, then clips it to the weight limit.
    pub fn step(&mut self, net: &mut FloatNetwork, grads: &Gradients) {
        self.steps += 1;
        let correction1 = (1.0 - f64::from(Self::BETA1).powf(self.steps as f64)) as f32;
        let correction2 = (1.0 - f64::from(Self::BETA2).powf(self.steps as f64)) as f32;
        let rate = self.learning_rate;
        let update = |weight: &mut f32, g: f32, m: &mut f32, v: &mut f32, decay: f32| {
            *m = Self::BETA1 * *m + (1.0 - Self::BETA1) * g;
            *v = Self::BETA2 * *v + (1.0 - Self::BETA2) * g * g;
            let (m_hat, v_hat) = (*m / correction1, *v / correction2);
            *weight -= rate * (m_hat / (v_hat.sqrt() + Self::EPSILON) + decay * *weight);
        };
        let (first, second) = (&mut self.first, &mut self.second);
        let groups = [
            (
                &mut net.input_weights,
                &grads.input_weights,
                &mut first.input_weights,
                &mut second.input_weights,
            ),
            (
                &mut net.input_biases,
                &grads.input_biases,
                &mut first.input_biases,
                &mut second.input_biases,
            ),
            (
                &mut net.output_weights,
                &grads.output_weights,
                &mut first.output_weights,
                &mut second.output_weights,
            ),
        ];
        for (weights, g, m, v) in groups {
            for i in 0..weights.len() {
                update(
                    &mut weights[i],
                    g[i],
                    &mut m[i],
                    &mut v[i],
                    self.weight_decay,
                );
                weights[i] = weights[i].clamp(-WEIGHT_LIMIT, WEIGHT_LIMIT);
            }
        }
        // The output bias: no decay and no clipping.
        let (m, v) = (&mut first.output_bias, &mut second.output_bias);
        update(&mut net.output_bias, grads.output_bias, m, v, 0.0);
    }
    // ANCHOR_END: step
}

#[cfg(test)]
mod tests {
    use cb2026_nnue::Activation;

    use super::*;

    fn net() -> FloatNetwork {
        FloatNetwork::zeros(32, Activation::SquaredClippedRelu, 146)
    }

    #[test]
    fn the_first_step_moves_each_weight_by_the_learning_rate_against_its_gradient() {
        let mut net = net();
        let mut grads = Gradients::zeros(32);
        grads.input_weights[3] = 5.0;
        grads.output_weights[1] = -0.001;
        grads.output_bias = 2.0;
        let mut adam = AdamW::new(32, 0.01, 0.0);
        adam.step(&mut net, &grads);
        // Adam's first step is ±learning rate, whatever the gradient's size.
        assert!((net.input_weights[3] + 0.01).abs() < 1e-6);
        assert!((net.output_weights[1] - 0.01).abs() < 1e-6);
        assert!((net.output_bias + 0.01).abs() < 1e-6);
        assert_eq!(net.input_weights[4], 0.0);
        assert_eq!(adam.steps(), 1);
    }

    #[test]
    fn weights_are_clipped_to_the_limit_but_the_output_bias_is_not() {
        let mut net = net();
        net.input_weights[0] = 1.979;
        net.output_bias = 3.0;
        let mut grads = Gradients::zeros(32);
        grads.input_weights[0] = -1.0;
        grads.output_bias = -1.0;
        let mut adam = AdamW::new(32, 0.5, 0.0);
        adam.step(&mut net, &grads);
        assert_eq!(net.input_weights[0], WEIGHT_LIMIT);
        assert!((net.output_bias - 3.5).abs() < 1e-6);
    }

    #[test]
    fn weight_decay_shrinks_weights_without_a_gradient() {
        let mut net = net();
        net.output_weights[0] = 1.0;
        let mut adam = AdamW::new(32, 0.1, 0.5);
        adam.step(&mut net, &Gradients::zeros(32));
        assert!((net.output_weights[0] - 0.95).abs() < 1e-6);
    }
}
