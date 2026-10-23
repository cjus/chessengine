// The first layer's weight gradients, first stage. A weight's gradient is its hidden value's
// slope, summed over every example and view in which its input is on. WGSL has no atomic
// addition for floats, so instead of each row adding into the weights it touches, the rows are
// grouped by input beforehand (`entries`, built on the CPU) and each weight sums its own group.
// The groups are very uneven: in a batch of 16,384 the average input is on in about 700 rows,
// but a king on its first square in over 11,000, and the invocation adding those up would keep
// the rest of the GPU waiting. So each group is cut into segments of up to SEGMENT rows, one
// invocation sums one hidden value over one segment, and input_gradients.wgsl adds up each
// input's segments. The order is fixed, so every run gives the same result.

@group(0) @binding(1) var<storage, read> d_hidden: array<f32>;
// The rows, grouped by input.
@group(0) @binding(2) var<storage, read> entries: array<u32>;
// Segment s is entries[segments[2s]..segments[2s + 1]].
@group(0) @binding(3) var<storage, read> segments: array<u32>;
// Per segment: its sum for each hidden value.
@group(0) @binding(4) var<storage, read_write> segment_sums: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let unit = id.x;
    let segment = id.y;
    if (unit >= sizes.hidden || segment >= sizes.segments) {
        return;
    }
    var sum = 0.0;
    for (var n = segments[2u * segment]; n < segments[2u * segment + 1u]; n++) {
        sum += d_hidden[entries[n] * sizes.hidden + unit];
    }
    segment_sums[segment * sizes.hidden + unit] = sum;
}
