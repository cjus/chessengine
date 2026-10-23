// The loss's slope with respect to each hidden value: the output's slope, times the value's
// output weight, times the activation's slope there.

@group(0) @binding(1) var<storage, read> params: array<f32>;
@group(0) @binding(2) var<storage, read> hidden: array<f32>;
@group(0) @binding(3) var<storage, read> outputs: array<f32>;
@group(0) @binding(4) var<storage, read_write> d_hidden: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let unit = id.x;
    let row = id.y;
    if (unit >= sizes.hidden || row >= 2u * sizes.batch) {
        return;
    }
    let example = row / 2u;
    let view = row % 2u;
    let at = row * sizes.hidden + unit;
    let weight = params[output_weights() + view * sizes.hidden + unit];
    d_hidden[at] = outputs[2u * example] * weight * slope(hidden[at]);
}
