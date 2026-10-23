// The output layer's gradients, first stage. A sum over the whole batch in one invocation per
// weight would leave the GPU nearly idle (a few hundred invocations, each 16,384 steps long),
// so the batch is split into chunks of 128 examples and each invocation sums one weight over
// one chunk; sum_partials.wgsl adds the chunks up. An output weight's gradient is the output's
// slope times its activation; the output bias's is the output's slope.

@group(0) @binding(1) var<storage, read> hidden: array<f32>;
@group(0) @binding(2) var<storage, read> outputs: array<f32>;
@group(0) @binding(3) var<storage, read_write> partials: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let j = id.x;
    let chunk = id.y;
    let width = 2u * sizes.hidden;
    if (j > width || chunk >= chunks()) {
        return;
    }
    let last = min((chunk + 1u) * CHUNK, sizes.batch);
    var sum = 0.0;
    for (var example = chunk * CHUNK; example < last; example++) {
        let d_output = outputs[2u * example];
        if (j == width) {
            sum += d_output;
        } else {
            sum += d_output * activate(hidden[example * width + j]);
        }
    }
    partials[chunk * partial_columns() + j] = sum;
}
