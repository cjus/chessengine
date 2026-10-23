// Adds the batch's losses to a running total, so the loss can be read back now and then
// rather than after every batch. One workgroup.

@group(0) @binding(1) var<storage, read> outputs: array<f32>;
@group(0) @binding(2) var<storage, read_write> total: array<f32>;

var<workgroup> partial: array<f32, 256>;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_index) lane: u32) {
    var sum = 0.0;
    for (var example = lane; example < sizes.batch; example += 256u) {
        sum += outputs[2u * example + 1u];
    }
    partial[lane] = sum;
    workgroupBarrier();
    for (var stride = 128u; stride > 0u; stride /= 2u) {
        if (lane < stride) {
            partial[lane] += partial[lane + stride];
        }
        workgroupBarrier();
    }
    if (lane == 0u) {
        total[0] += partial[0];
    }
}
