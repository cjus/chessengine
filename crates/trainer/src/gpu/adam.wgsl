// AdamW's step for every parameter at once, as the CPU's `AdamW::step`: update the moments,
// step against the bias-corrected ratio with weight decay, and clip, except for the output
// bias, which is neither decayed nor clipped.

@group(0) @binding(1) var<storage, read_write> params: array<f32>;
@group(0) @binding(2) var<storage, read> grads: array<f32>;
@group(0) @binding(3) var<storage, read_write> first: array<f32>;
@group(0) @binding(4) var<storage, read_write> second: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if (k >= sizes.total) {
        return;
    }
    let g = grads[k];
    let m = BETA1 * first[k] + (1.0 - BETA1) * g;
    let v = BETA2 * second[k] + (1.0 - BETA2) * g * g;
    first[k] = m;
    second[k] = v;
    let is_output_bias = k == output_bias();
    let decay = select(sizes.weight_decay, 0.0, is_output_bias);
    let m_hat = m / sizes.correction1;
    let v_hat = v / sizes.correction2;
    var weight = params[k];
    weight -= sizes.learning_rate * (m_hat / (sqrt(v_hat) + EPSILON) + decay * weight);
    if (!is_output_bias) {
        weight = clamp(weight, -sizes.weight_limit, sizes.weight_limit);
    }
    params[k] = weight;
}
