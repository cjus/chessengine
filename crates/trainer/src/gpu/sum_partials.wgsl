// The second stage: adds up each gradient's partial sums, chunk by chunk in a fixed order,
// and stores it: the output weights, the output bias and the first layer's biases.

@group(0) @binding(1) var<storage, read> partials: array<f32>;
@group(0) @binding(2) var<storage, read_write> grads: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let j = id.x;
    let width = 2u * sizes.hidden;
    if (j >= partial_columns()) {
        return;
    }
    var gradient = 0.0;
    for (var chunk = 0u; chunk < chunks(); chunk++) {
        gradient += partials[chunk * partial_columns() + j];
    }
    if (j < width) {
        grads[output_weights() + j] = gradient;
    } else if (j == width) {
        grads[output_bias()] = gradient;
    } else {
        grads[input_biases() + j - width - 1u] = gradient;
    }
}
