// Shared by every kernel: the sizes and settings of this batch, the parameters' layout in
// their buffer, and the activation. Prepended to each kernel's source.

struct Sizes {
    // The hidden layer's size, per view.
    hidden: u32,
    // Examples in this batch.
    batch: u32,
    // 0: CReLU, 1: SCReLU.
    activation: u32,
    // Parameters in all: 771 × hidden + 1.
    total: u32,
    // 1 / batch: the loss is the batch's mean.
    inv_batch: f32,
    // AdamW's settings for this step.
    learning_rate: f32,
    weight_decay: f32,
    // 1 − β1^t and 1 − β2^t, the bias corrections.
    correction1: f32,
    correction2: f32,
    // Weights are clipped to ± this after each step.
    weight_limit: f32,
    // Segments of the input lists in this batch (see input_partials.wgsl).
    segments: u32,
    padding: u32,
}

@group(0) @binding(0) var<uniform> sizes: Sizes;

const INPUTS: u32 = 768u;
const MAX_ACTIVE: u32 = 32u;
// Marks the end of an example's inputs when it has fewer than 32.
const NONE: u32 = 0xffffffffu;
// Examples per chunk when sums over the batch are split up (see output_partials.wgsl).
const CHUNK: u32 = 128u;
const BETA1: f32 = 0.9;
const BETA2: f32 = 0.999;
const EPSILON: f32 = 1e-8;

// The parameters, one after another in one buffer: the first layer's weights (one row of
// `hidden` per input), its biases, the output weights (the side to move's half first), and
// the output bias. The same layout holds the gradients and AdamW's moments.
fn input_biases() -> u32 {
    return INPUTS * sizes.hidden;
}
fn output_weights() -> u32 {
    return (INPUTS + 1u) * sizes.hidden;
}
fn output_bias() -> u32 {
    return (INPUTS + 3u) * sizes.hidden;
}

// The activation: clamp to 0..1, squared for SCReLU.
fn activate(x: f32) -> f32 {
    let c = clamp(x, 0.0, 1.0);
    if (sizes.activation == 1u) {
        return c * c;
    }
    return c;
}

// The activation's slope: zero where the clamp holds the value at 0 or 1.
fn slope(x: f32) -> f32 {
    if (x <= 0.0 || x >= 1.0) {
        return 0.0;
    }
    if (sizes.activation == 1u) {
        return 2.0 * x;
    }
    return 1.0;
}

// The chunks the batch is split into for the sums over the batch.
fn chunks() -> u32 {
    return (sizes.batch + CHUNK - 1u) / CHUNK;
}

// Columns of the partial sums: the 2 × hidden output weights, the output bias, then the
// hidden first-layer biases.
fn partial_columns() -> u32 {
    return 3u * sizes.hidden + 1u;
}
