// The output, the loss and its slope: one workgroup per example. Its 64 invocations each add up
// part of the activations times the output weights, then combine their sums in shared memory.

@group(0) @binding(1) var<storage, read> params: array<f32>;
@group(0) @binding(2) var<storage, read> hidden: array<f32>;
@group(0) @binding(3) var<storage, read> targets: array<f32>;
// Per example: the slope of the batch's mean loss with respect to the output, and the loss.
@group(0) @binding(4) var<storage, read_write> outputs: array<f32>;

var<workgroup> partial: array<f32, 64>;

@compute @workgroup_size(64)
fn main(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let example = group.x;
    let width = 2u * sizes.hidden;
    var sum = 0.0;
    for (var j = lane; j < width; j += 64u) {
        sum += activate(hidden[example * width + j]) * params[output_weights() + j];
    }
    partial[lane] = sum;
    workgroupBarrier();
    for (var stride = 32u; stride > 0u; stride /= 2u) {
        if (lane < stride) {
            partial[lane] += partial[lane + stride];
        }
        workgroupBarrier();
    }
    if (lane == 0u) {
        let output = partial[0] + params[output_bias()];
        let p = 1.0 / (1.0 + exp(-output));
        let error = p - targets[example];
        outputs[2u * example] = 2.0 * error * p * (1.0 - p) * sizes.inv_batch;
        outputs[2u * example + 1u] = error * error;
    }
}
