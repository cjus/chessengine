//! The forward pass, the loss and the backward pass, worked out by hand.
//!
//! ## Forward
//!
//! For each view `v` (side to move first), each hidden value is its bias plus the weights of the
//! inputs that are on, and goes through the activation `f` (CReLU or SCReLU):
//!
//! ```text
//! h[v][i] = bias[i] + Σ W[input][i]     over the inputs on from view v
//! y       = out_bias + Σ f(h[v][i]) × out[v][i]     over both views and every i
//! ```
//!
//! ## Loss
//!
//! With `p = σ(y)` and the example's target `t`, the loss is `(p − t)²`, the squared error in
//! expected result. Its slope with respect to the output, by the chain rule, is
//!
//! ```text
//! dL/dy = 2 (p − t) × p (1 − p)          (σ's slope is σ (1 − σ))
//! ```
//!
//! ## Backward
//!
//! Each weight's gradient is how much the loss would change per unit that weight changes. Going
//! back through the formulas above:
//!
//! ```text
//! dL/d out_bias  = dL/dy
//! dL/d out[v][i] = dL/dy × f(h[v][i])
//! dL/d h[v][i]   = dL/dy × out[v][i] × f'(h[v][i])
//! dL/d bias[i]   = Σ over views of dL/d h[v][i]
//! dL/d W[input][i] += dL/d h[v][i]       for each input on from view v
//! ```
//!
//! `f'` is the activation's slope: 0 where the clipping holds the value at 0 or 1, otherwise 1
//! for CReLU and `2h` for SCReLU. Only the rows of inputs that are on get a gradient, about 32 of
//! 768, which is what makes training sparse and fast. Both views share the first layer, so a row
//! can collect gradient from each.
//!
//! The tests check every formula against finite differences: nudge a weight a little each way
//! and see how the loss moves.

use cb2026_nnue::{Activation, FloatNetwork, INPUTS};

use crate::example::{Example, sigmoid};

/// The gradient of the loss with respect to every weight, in the network's shapes. Also used for
/// AdamW's moment estimates, which have the same shapes.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradients {
    /// One per first-layer weight: [`INPUTS`] rows of `hidden`.
    pub input_weights: Vec<f32>,
    /// One per first-layer bias.
    pub input_biases: Vec<f32>,
    /// One per output weight: the side to move's half, then the other side's.
    pub output_weights: Vec<f32>,
    /// The output bias's.
    pub output_bias: f32,
}

impl Gradients {
    /// All zero, for a network with a hidden layer of `hidden`.
    pub fn zeros(hidden: usize) -> Gradients {
        Gradients {
            input_weights: vec![0.0; INPUTS * hidden],
            input_biases: vec![0.0; hidden],
            output_weights: vec![0.0; 2 * hidden],
            output_bias: 0.0,
        }
    }

    /// Sets every gradient back to zero.
    pub fn clear(&mut self) {
        self.input_weights.fill(0.0);
        self.input_biases.fill(0.0);
        self.output_weights.fill(0.0);
        self.output_bias = 0.0;
    }

    /// Adds `other`'s gradients to these (to combine the threads' shares of a batch).
    pub fn add(&mut self, other: &Gradients) {
        let add = |a: &mut [f32], b: &[f32]| a.iter_mut().zip(b).for_each(|(a, b)| *a += b);
        add(&mut self.input_weights, &other.input_weights);
        add(&mut self.input_biases, &other.input_biases);
        add(&mut self.output_weights, &other.output_weights);
        self.output_bias += other.output_bias;
    }

    /// Multiplies every gradient by `factor` (to turn a batch's sum into its mean).
    pub fn scale(&mut self, factor: f32) {
        let scale = |a: &mut [f32]| a.iter_mut().for_each(|a| *a *= factor);
        scale(&mut self.input_weights);
        scale(&mut self.input_biases);
        scale(&mut self.output_weights);
        self.output_bias *= factor;
    }
}

/// Working space for one example, reused from one to the next.
#[derive(Clone, Debug)]
pub struct Scratch {
    /// The hidden layer before the activation, both views: the side to move's half first.
    hidden: Vec<f32>,
    /// The loss's gradient with respect to one view's hidden values.
    d_hidden: Vec<f32>,
}

impl Scratch {
    /// Space for a network with a hidden layer of `hidden`.
    pub fn new(hidden: usize) -> Scratch {
        Scratch {
            hidden: vec![0.0; 2 * hidden],
            d_hidden: vec![0.0; hidden],
        }
    }
}

// ANCHOR: forward
/// The network's output for `example`, before the sigmoid. Leaves the hidden layer in
/// `scratch` for [`backward`].
pub fn forward(net: &FloatNetwork, example: &Example, scratch: &mut Scratch) -> f32 {
    let h = net.hidden;
    let mut output = net.output_bias;
    for view in 0..2 {
        let values = &mut scratch.hidden[view * h..(view + 1) * h];
        values.copy_from_slice(&net.input_biases);
        for &input in example.inputs(view) {
            let row = &net.input_weights[usize::from(input) * h..(usize::from(input) + 1) * h];
            values.iter_mut().zip(row).for_each(|(v, w)| *v += w);
        }
        let weights = &net.output_weights[view * h..(view + 1) * h];
        output += values
            .iter()
            .zip(weights)
            .map(|(&v, w)| net.activate(v) * w)
            .sum::<f32>();
    }
    output
}
// ANCHOR_END: forward

// ANCHOR: loss
/// The loss for an output and a target, `(σ(output) − target)²`, and its slope with respect to
/// the output.
pub fn loss(output: f32, target: f32) -> (f32, f32) {
    let p = sigmoid(output);
    let error = p - target;
    (error * error, 2.0 * error * p * (1.0 - p))
}
// ANCHOR_END: loss

/// The activation's slope at a hidden value: zero where clipping holds it at 0 or 1.
pub fn slope(activation: Activation, value: f32) -> f32 {
    if value <= 0.0 || value >= 1.0 {
        return 0.0;
    }
    match activation {
        Activation::ClippedRelu => 1.0,
        Activation::SquaredClippedRelu => 2.0 * value,
    }
}

// ANCHOR: backward
/// Adds `example`'s gradients to `grads`, given the hidden layer [`forward`] left in `scratch`
/// and `d_output`, the loss's slope with respect to the output.
pub fn backward(
    net: &FloatNetwork,
    example: &Example,
    scratch: &mut Scratch,
    d_output: f32,
    grads: &mut Gradients,
) {
    let h = net.hidden;
    grads.output_bias += d_output;
    for view in 0..2 {
        let values = &scratch.hidden[view * h..(view + 1) * h];
        let weights = &net.output_weights[view * h..(view + 1) * h];
        let output_grads = &mut grads.output_weights[view * h..(view + 1) * h];
        for i in 0..h {
            output_grads[i] += d_output * net.activate(values[i]);
            scratch.d_hidden[i] = d_output * weights[i] * slope(net.activation, values[i]);
        }
        let d_hidden = &scratch.d_hidden;
        grads
            .input_biases
            .iter_mut()
            .zip(d_hidden)
            .for_each(|(g, d)| *g += d);
        for &input in example.inputs(view) {
            let row =
                &mut grads.input_weights[usize::from(input) * h..(usize::from(input) + 1) * h];
            row.iter_mut().zip(d_hidden).for_each(|(g, d)| *g += d);
        }
    }
}
// ANCHOR_END: backward

#[cfg(test)]
mod tests {
    use cb2026_nnue::Record;

    use super::*;
    use crate::example::Targets;
    use crate::test_support::{random_network, random_records};

    const TARGETS: Targets = Targets {
        eval_scale: 146.0,
        result_weight: 0.25,
    };

    #[test]
    fn the_forward_pass_matches_the_float_network() {
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            let net = random_network(64, activation, 1);
            let mut scratch = Scratch::new(64);
            for record in random_records(2, 10) {
                let example = Example::from_record(&record, TARGETS);
                let ours = forward(&net, &example, &mut scratch);
                let reference = net.output(&record.to_position());
                assert!((ours - reference).abs() < 1e-4, "{ours} vs {reference}");
            }
        }
    }

    /// The mean loss over `examples`, computed independently in `f64`. In `f32`, σ(output) near
    /// ½ is only exact to about 6 × 10⁻⁸, which is a few percent of the loss changes a small nudge
    /// makes; `f64` measures them exactly enough.
    fn mean_loss(net: &FloatNetwork, examples: &[Example]) -> f64 {
        let h = net.hidden;
        let activate = |x: f64| {
            let c = x.clamp(0.0, 1.0);
            match net.activation {
                Activation::ClippedRelu => c,
                Activation::SquaredClippedRelu => c * c,
            }
        };
        let total: f64 = examples
            .iter()
            .map(|example| {
                let mut output = f64::from(net.output_bias);
                for view in 0..2 {
                    for i in 0..h {
                        let mut value = f64::from(net.input_biases[i]);
                        for &input in example.inputs(view) {
                            value += f64::from(net.input_weights[usize::from(input) * h + i]);
                        }
                        output += activate(value) * f64::from(net.output_weights[view * h + i]);
                    }
                }
                let p = 1.0 / (1.0 + (-output).exp());
                (p - f64::from(example.target)).powi(2)
            })
            .sum();
        total / examples.len() as f64
    }

    /// Checks one weight's gradient against the loss's change when that weight is nudged by
    /// ±`step`. `weight` picks the weight in a network.
    #[track_caller]
    fn check(
        net: &FloatNetwork,
        examples: &[Example],
        analytic: f32,
        weight: impl Fn(&mut FloatNetwork) -> &mut f32,
    ) {
        let step = STEP;
        let mut plus = net.clone();
        *weight(&mut plus) += step;
        let mut minus = net.clone();
        *weight(&mut minus) -= step;
        let numeric =
            (mean_loss(&plus, examples) - mean_loss(&minus, examples)) / (2.0 * f64::from(step));
        let analytic = f64::from(analytic);
        assert!(
            (numeric - analytic).abs() <= 1e-3 * analytic.abs().max(numeric.abs()) + 1e-9,
            "analytic {analytic:e}, numeric {numeric:e}"
        );
    }

    /// How far a weight is nudged to estimate its gradient.
    const STEP: f32 = 1e-3;

    /// Whether hidden value `unit` comes within twice the step of 0 or 1 in any example, from
    /// either view. There the activation's slope jumps, so a nudge across it measures the wrong
    /// slope: such units are left out of the check.
    fn near_a_corner(net: &FloatNetwork, examples: &[Example], unit: usize) -> bool {
        let mut scratch = Scratch::new(net.hidden);
        examples.iter().any(|example| {
            forward(net, example, &mut scratch);
            (0..2).any(|view| {
                let h = scratch.hidden[view * net.hidden + unit];
                h.abs() < 2.0 * STEP || (h - 1.0).abs() < 2.0 * STEP
            })
        })
    }

    #[test]
    fn every_gradient_matches_finite_differences() {
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            let net = random_network(32, activation, 3);
            // A few examples, spread through a random game: few enough that most hidden values
            // stay clear of the corners.
            let examples: Vec<Example> = random_records(4, 1)
                .iter()
                .step_by(10)
                .take(6)
                .map(|r: &Record| Example::from_record(r, TARGETS))
                .collect();
            let mut grads = Gradients::zeros(32);
            let mut scratch = Scratch::new(32);
            for example in &examples {
                let (_, d_output) = loss(forward(&net, example, &mut scratch), example.target);
                backward(&net, example, &mut scratch, d_output, &mut grads);
            }
            grads.scale(1.0 / examples.len() as f32);

            check(&net, &examples, grads.output_bias, |n| &mut n.output_bias);
            let units: Vec<usize> = (0..32)
                .filter(|&u| !near_a_corner(&net, &examples, u))
                .collect();
            assert!(
                units.len() >= 16,
                "only {} units clear of a corner",
                units.len()
            );
            for &i in &units {
                check(&net, &examples, grads.output_weights[i], |n| {
                    &mut n.output_weights[i]
                });
                check(&net, &examples, grads.output_weights[32 + i], |n| {
                    &mut n.output_weights[32 + i]
                });
                check(&net, &examples, grads.input_biases[i], |n| {
                    &mut n.input_biases[i]
                });
                // Rows of inputs that are on: the first example's first inputs from each view.
                for view in 0..2 {
                    for &input in &examples[0].inputs(view)[..3] {
                        let at = usize::from(input) * 32 + i;
                        check(&net, &examples, grads.input_weights[at], |n| {
                            &mut n.input_weights[at]
                        });
                    }
                }
            }
            // A row no example uses gets no gradient.
            let unused = (0..INPUTS)
                .find(|&input| {
                    examples
                        .iter()
                        .all(|e| (0..2).all(|v| !e.inputs(v).contains(&(input as u16))))
                })
                .unwrap();
            assert!(
                grads.input_weights[unused * 32..(unused + 1) * 32]
                    .iter()
                    .all(|&g| g == 0.0)
            );
        }
    }

    #[test]
    fn the_slope_is_zero_where_the_activation_is_clipped() {
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            assert_eq!(slope(activation, -0.5), 0.0);
            assert_eq!(slope(activation, 1.5), 0.0);
        }
        assert_eq!(slope(Activation::ClippedRelu, 0.5), 1.0);
        assert_eq!(slope(Activation::SquaredClippedRelu, 0.5), 1.0);
        assert_eq!(slope(Activation::SquaredClippedRelu, 0.25), 0.5);
    }

    #[test]
    fn gradients_add_and_scale() {
        let mut a = Gradients::zeros(32);
        a.input_weights[7] = 1.0;
        a.output_bias = 2.0;
        let mut b = a.clone();
        b.add(&a);
        b.scale(0.25);
        assert_eq!(b.input_weights[7], 0.5);
        assert_eq!(b.output_bias, 1.0);
        b.clear();
        assert_eq!(b, Gradients::zeros(32));
    }
}
