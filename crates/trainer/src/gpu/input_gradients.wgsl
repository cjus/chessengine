// The first layer's weight gradients, second stage: adds up the sums of each input's segments
// (see input_partials.wgsl), in a fixed order. One invocation per weight.

@group(0) @binding(1) var<storage, read> segment_sums: array<f32>;
// Input i's segments are first_segment[i]..first_segment[i + 1].
@group(0) @binding(2) var<storage, read> first_segment: array<u32>;
@group(0) @binding(3) var<storage, read_write> grads: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let unit = id.x;
    let input = id.y;
    if (unit >= sizes.hidden || input >= INPUTS) {
        return;
    }
    var gradient = 0.0;
    for (var s = first_segment[input]; s < first_segment[input + 1u]; s++) {
        gradient += segment_sums[s * sizes.hidden + unit];
    }
    grads[input * sizes.hidden + unit] = gradient;
}
