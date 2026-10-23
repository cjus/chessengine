// The first layer's bias gradients, first stage: each hidden value's slope, summed over both
// views of one chunk's examples (see output_partials.wgsl).

@group(0) @binding(1) var<storage, read> d_hidden: array<f32>;
@group(0) @binding(2) var<storage, read_write> partials: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let unit = id.x;
    let chunk = id.y;
    if (unit >= sizes.hidden || chunk >= chunks()) {
        return;
    }
    let last = 2u * min((chunk + 1u) * CHUNK, sizes.batch);
    var sum = 0.0;
    for (var row = 2u * chunk * CHUNK; row < last; row++) {
        sum += d_hidden[row * sizes.hidden + unit];
    }
    partials[chunk * partial_columns() + 2u * sizes.hidden + 1u + unit] = sum;
}
