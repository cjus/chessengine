// The forward pass's first layer: each hidden value is its bias plus the weights of the inputs
// that are on. One invocation per hidden value of each example's view.

@group(0) @binding(1) var<storage, read> params: array<f32>;
// Per example and view (row = example × 2 + view): up to 32 inputs, then NONE.
@group(0) @binding(2) var<storage, read> inputs: array<u32>;
// Per row: the hidden layer before the activation.
@group(0) @binding(3) var<storage, read_write> hidden: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let unit = id.x;
    let row = id.y;
    if (unit >= sizes.hidden || row >= 2u * sizes.batch) {
        return;
    }
    var value = params[input_biases() + unit];
    for (var k = 0u; k < MAX_ACTIVE; k++) {
        let input = inputs[row * MAX_ACTIVE + k];
        if (input == NONE) {
            break;
        }
        value += params[input * sizes.hidden + unit];
    }
    hidden[row * sizes.hidden + unit] = value;
}
