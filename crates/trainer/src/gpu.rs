//! The trainer's GPU backend (the `gpu` feature): the same forward pass, loss, backward pass
//! and AdamW step as the CPU's, as compute kernels on the GPU.
//!
//! The kernels are written in WGSL, the WebGPU shading language, and run through wgpu: on
//! Metal on the Mac, on Vulkan or DirectX 12 elsewhere. Each is a small file in `src/gpu/`,
//! one step of the CPU version's formulas (see [`crate::backprop`] and [`crate::adam`]):
//!
//! | Kernel | What it computes | One invocation per |
//! | --- | --- | --- |
//! | `forward_hidden` | each hidden value: bias plus the rows of the inputs that are on | hidden value of each example's view |
//! | `forward_output` | the output, the loss and its slope with respect to the output | example (a workgroup of 64) |
//! | `backward_hidden` | each hidden value's slope: output slope × output weight × activation slope | hidden value of each example's view |
//! | `output_partials` | the output weights' and bias's gradients, summed over a chunk of 128 examples | output weight and chunk |
//! | `bias_partials` | the first layer's bias gradients, summed over a chunk | hidden value and chunk |
//! | `sum_partials` | the chunks' sums added up into the gradients | output weight or bias |
//! | `input_partials` | the first layer's weight gradients, summed over a segment of up to 256 rows | hidden value and segment |
//! | `input_gradients` | the segments' sums added up into the first layer's weight gradients | first-layer weight |
//! | `adam` | AdamW's step and the clipping | parameter |
//! | `sum_losses` | the batch's losses added to a running total | (one workgroup) |
//!
//! ## The sparse gradient without atomics
//!
//! A first-layer weight's gradient collects one term from every example and view in which its
//! input is on. On a GPU the natural way is for each example to add into the rows it touches,
//! but many examples touch the same rows at once, and WGSL has no atomic addition for floats.
//! So the CPU sorts each batch's rows by input first (a counting sort: `offsets` and
//! `entries`), and then each weight adds up its own list with nobody else writing to it. The
//! lists are in a fixed order, so the GPU gives the same result every time. The sorting, with
//! the decoding of the records, is shared among the trainer's threads (see `prepare`).
//!
//! The lists are very uneven. In a batch of 16,384 the average input is on in about 700 of the
//! 32,768 rows, but a king on its first square is on in over 11,000. With one invocation per
//! weight looping over its whole list, the kernel took as long as the longest list, whatever the
//! network's size: on the M1 Max a network of 512 trained no faster than one of 1,024. So the CPU
//! also cuts each list into segments of up to 256 rows, `input_partials` sums each segment, and
//! `input_gradients` adds up each input's segments.
//!
//! ## Sums over the batch
//!
//! The output layer's and the biases' gradients are sums over the whole batch. One invocation
//! per weight, each looping over 16,384 examples, left the GPU nearly idle: those two kernels
//! took 25 of a batch's 35 milliseconds on the M1 Max. So the batch is split into chunks of 128
//! examples, an invocation sums one weight over one chunk, and a second kernel adds the
//! chunks up, in a fixed order.
//!
//! ## Memory
//!
//! All the parameters live in one buffer, one after another (first-layer weights, biases,
//! output weights, output bias), and so do the gradients and AdamW's two moments. They stay on
//! the GPU for the whole run: only each batch's inputs and targets go up, and the network comes
//! back only for a checkpoint, at the end of an epoch. The losses are added up on the GPU and
//! read back only when they are reported.
//!
//! ## Speed
//!
//! On the M1 Max (32 GPU cores), on 2.8 million records, in records a second: 5.3 million at a
//! hidden layer of 256 (the CPU trainer on 8 threads: 2.3 million), 3.0 million at 512 (1.1
//! million) and 1.4 million at 1,024 (0.54 million). The GPU's work is the limit at every size:
//! the batch preparation on 8 threads takes 0.8 milliseconds, against 3.1 for a whole batch at
//! 256. Other Macs differ in cores, GPU and memory bandwidth: `scripts/mac-trainer-speed.sh`
//! measures both backends on the Mac it runs on.
//!
//! ## Testing
//!
//! The tests run the GPU and the CPU on the same batch and compare: the gradients, an AdamW
//! step, and a short training run. Additions happen in a different order on the GPU, so results
//! agree to rounding rather than bit for bit. Without a GPU the tests say so and pass.

use std::borrow::Cow;
use std::thread;

use cb2026_nnue::float::WEIGHT_LIMIT;
use cb2026_nnue::{Activation, FloatNetwork, INPUTS};

use crate::adam::AdamW;
use crate::backprop::Gradients;
use crate::checkpoint::Checkpoint;
use crate::data::Dataset;
use crate::example::{Example, MAX_ACTIVE, Targets};
use crate::train::{Backend, Settings};

/// Invocations per workgroup in the kernels that work element by element.
const WORKGROUP: u32 = 64;
/// Marks the end of an example's inputs (`NONE` in the kernels).
const NONE: u32 = u32::MAX;
/// The definitions every kernel shares.
const COMMON: &str = include_str!("gpu/common.wgsl");
/// The size of the `Sizes` uniform, in 32-bit words.
const SIZES_WORDS: usize = 12;
/// Examples per chunk in the sums over the batch (`CHUNK` in the kernels).
const CHUNK: usize = 128;
/// Rows per segment of an input's list in the first layer's weight gradients
/// (`input_partials.wgsl`).
const SEGMENT: usize = 256;

/// A compute kernel, ready to run: its pipeline and the buffers bound to it.
struct Kernel {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
}

/// Every kernel, in the order a training step runs them.
struct Kernels {
    forward_hidden: Kernel,
    forward_output: Kernel,
    backward_hidden: Kernel,
    output_partials: Kernel,
    bias_partials: Kernel,
    sum_partials: Kernel,
    input_partials: Kernel,
    input_gradients: Kernel,
    adam: Kernel,
    sum_losses: Kernel,
}

/// The GPU's buffers.
struct Buffers {
    sizes: wgpu::Buffer,
    params: wgpu::Buffer,
    grads: wgpu::Buffer,
    first: wgpu::Buffer,
    second: wgpu::Buffer,
    inputs: wgpu::Buffer,
    targets: wgpu::Buffer,
    hidden: wgpu::Buffer,
    d_hidden: wgpu::Buffer,
    outputs: wgpu::Buffer,
    partials: wgpu::Buffer,
    entries: wgpu::Buffer,
    segments: wgpu::Buffer,
    first_segment: wgpu::Buffer,
    segment_sums: wgpu::Buffer,
    total: wgpu::Buffer,
}

/// Training on the GPU.
pub struct GpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: String,
    hidden: usize,
    activation: Activation,
    eval_scale: u32,
    targets: Targets,
    weight_decay: f32,
    steps: u64,
    max_batch: usize,
    /// Threads that prepare each batch on the CPU.
    threads: usize,
    buffers: Buffers,
    kernels: Kernels,
}

/// The number of parameters of a network with a hidden layer of `hidden`: 771 × hidden + 1.
fn parameter_count(hidden: usize) -> usize {
    INPUTS * hidden + hidden + 2 * hidden + 1
}

/// A network's (or gradients') numbers in the GPU's order, one after another.
fn pack(
    input_weights: &[f32],
    input_biases: &[f32],
    output_weights: &[f32],
    output_bias: f32,
) -> Vec<f32> {
    let mut values =
        Vec::with_capacity(input_weights.len() + input_biases.len() + output_weights.len() + 1);
    values.extend_from_slice(input_weights);
    values.extend_from_slice(input_biases);
    values.extend_from_slice(output_weights);
    values.push(output_bias);
    values
}

/// The GPU's order unpacked into gradients' shapes (also the network's and the moments').
fn unpack(values: &[f32], hidden: usize) -> Gradients {
    let (input_weights, rest) = values.split_at(INPUTS * hidden);
    let (input_biases, rest) = rest.split_at(hidden);
    let (output_weights, rest) = rest.split_at(2 * hidden);
    Gradients {
        input_weights: input_weights.to_vec(),
        input_biases: input_biases.to_vec(),
        output_weights: output_weights.to_vec(),
        output_bias: rest[0],
    }
}

/// Bytes of `n` 32-bit numbers.
fn bytes(n: usize) -> u64 {
    (4 * n.max(1)) as u64
}

/// The most segments a batch of `batch` can have: each input's list of `n` rows has
/// ⌈n / SEGMENT⌉, which is at most n / SEGMENT + 1.
fn max_segments(batch: usize) -> usize {
    (2 * batch * MAX_ACTIVE).div_ceil(SEGMENT) + INPUTS
}

/// A batch made ready for the GPU (see [`prepare`]).
#[derive(Debug, PartialEq)]
struct Prepared {
    /// Per row (example × 2 + view): up to `MAX_ACTIVE` inputs, then `NONE`.
    inputs: Vec<u32>,
    /// Per example: what `σ(output)` should be.
    targets: Vec<f32>,
    /// The rows, grouped by input and in row order within each input.
    entries: Vec<u32>,
    /// Segment s is `entries[segments[2s]..segments[2s + 1]]`, up to `SEGMENT` rows.
    segments: Vec<u32>,
    /// Input i's segments are `first_segment[i]..first_segment[i + 1]`.
    first_segment: Vec<u32>,
}

/// One thread's share of a batch, its rows grouped by input: input i's are
/// `entries[offsets[i]..offsets[i + 1]]`.
struct Share {
    offsets: Vec<u32>,
    entries: Vec<u32>,
}

/// Turns the records `batch` of `data` into what the GPU trains on, on `threads` threads.
///
/// Each thread decodes a share of the examples and groups its own rows by input; the shares are
/// then joined input by input, in order. So any number of threads gives the same lists, and the
/// GPU the same network. On one thread this took about 4 milliseconds a batch on the M1 Max,
/// more than the GPU's own work at a hidden layer of 256 (3.5), so the GPU waited for it; on 8
/// threads it takes 0.8, and training at 256 went from 2.7 to 5.3 million records a second.
fn prepare(data: &Dataset, batch: &[u32], targets: Targets, threads: usize) -> Prepared {
    let mut inputs = vec![NONE; 2 * batch.len() * MAX_ACTIVE];
    let mut target_values = vec![0.0; batch.len()];
    let share = batch.len().div_ceil(threads.max(1)).max(1);
    let shares: Vec<Share> = thread::scope(|scope| {
        let handles: Vec<_> = batch
            .chunks(share)
            .zip(inputs.chunks_mut(2 * share * MAX_ACTIVE))
            .zip(target_values.chunks_mut(share))
            .enumerate()
            .map(|(n, ((examples, inputs), values))| {
                let first_row = 2 * n * share;
                scope.spawn(move || {
                    prepare_share(data, examples, targets, first_row, inputs, values)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a thread preparing a batch panicked"))
            .collect()
    });
    // ANCHOR: group_by_input
    // Input i's rows are each share's rows for input i, the shares in order: the same lists, in
    // the same row order, whatever the number of threads.
    let mut offsets = Vec::with_capacity(INPUTS + 1);
    let mut entries = Vec::with_capacity(shares.iter().map(|share| share.entries.len()).sum());
    for i in 0..INPUTS {
        offsets.push(entries.len() as u32);
        for share in &shares {
            let rows = share.offsets[i] as usize..share.offsets[i + 1] as usize;
            entries.extend_from_slice(&share.entries[rows]);
        }
    }
    offsets.push(entries.len() as u32);
    // Each input's list cut into segments of up to SEGMENT rows.
    let mut segments = Vec::new();
    let mut first_segment = Vec::with_capacity(INPUTS + 1);
    for i in 0..INPUTS {
        first_segment.push((segments.len() / 2) as u32);
        for start in (offsets[i]..offsets[i + 1]).step_by(SEGMENT) {
            segments.extend([start, offsets[i + 1].min(start + SEGMENT as u32)]);
        }
    }
    first_segment.push((segments.len() / 2) as u32);
    // ANCHOR_END: group_by_input
    Prepared {
        inputs,
        targets: target_values,
        entries,
        segments,
        first_segment,
    }
}

/// Decodes `examples` into `inputs` (their rows, the first being row `first_row` of the batch)
/// and `values` (their targets), and groups their rows by input.
fn prepare_share(
    data: &Dataset,
    examples: &[u32],
    targets: Targets,
    first_row: usize,
    inputs: &mut [u32],
    values: &mut [f32],
) -> Share {
    // How many rows each input is on in, shifted by one for the prefix sums below.
    let mut offsets = vec![0u32; INPUTS + 1];
    for (e, &i) in examples.iter().enumerate() {
        let example = Example::from_record(&data.record(i as usize), targets);
        for view in 0..2 {
            let row = &mut inputs[(2 * e + view) * MAX_ACTIVE..][..MAX_ACTIVE];
            for (slot, &input) in row.iter_mut().zip(example.inputs(view)) {
                *slot = u32::from(input);
                offsets[usize::from(input) + 1] += 1;
            }
        }
        values[e] = example.target;
    }
    // ANCHOR: count_sort
    // A counting sort: input i's rows will be entries[offsets[i]..offsets[i + 1]], in row
    // order.
    for i in 1..=INPUTS {
        offsets[i] += offsets[i - 1];
    }
    let mut next = offsets.clone();
    let mut entries = vec![0u32; offsets[INPUTS] as usize];
    for (row, inputs) in inputs.chunks_exact(MAX_ACTIVE).enumerate() {
        for &input in inputs.iter().take_while(|&&input| input != NONE) {
            entries[next[input as usize] as usize] = (first_row + row) as u32;
            next[input as usize] += 1;
        }
    }
    // ANCHOR_END: count_sort
    Share { offsets, entries }
}

impl GpuBackend {
    /// Training from `checkpoint` on the first GPU found, in batches of up to
    /// `settings.batch_size`. Fails if there is no GPU or the batches don't fit in its memory.
    pub fn new(checkpoint: Checkpoint, settings: &Settings) -> Result<GpuBackend, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU found: {e}"))?;
        let limits = adapter.limits();
        let (hidden, max_batch) = (checkpoint.net.hidden, settings.batch_size);
        // The largest buffers hold a hidden layer for both views of every example, or one for
        // each segment.
        let largest = bytes(2 * max_batch.max(max_segments(max_batch)) * hidden);
        let most_workgroups = limits.max_compute_workgroups_per_dimension as usize;
        if largest
            > limits
                .max_storage_buffer_binding_size
                .min(limits.max_buffer_size)
            || 2 * max_batch > most_workgroups
            || max_segments(max_batch) > most_workgroups
        {
            return Err(format!(
                "batches of {max_batch} with a hidden layer of {hidden} don't fit on this GPU; try a smaller --batch"
            ));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("cb2026 trainer"),
            required_limits: limits,
            ..Default::default()
        }))
        .map_err(|e| format!("can't open the GPU: {e}"))?;

        let total = parameter_count(hidden);
        let usage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC;
        let buffer = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let buffers = Buffers {
            sizes: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sizes"),
                size: bytes(SIZES_WORDS),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            params: buffer("params", bytes(total)),
            grads: buffer("grads", bytes(total)),
            first: buffer("first moments", bytes(total)),
            second: buffer("second moments", bytes(total)),
            inputs: buffer("inputs", bytes(2 * max_batch * MAX_ACTIVE)),
            targets: buffer("targets", bytes(max_batch)),
            hidden: buffer("hidden", bytes(2 * max_batch * hidden)),
            d_hidden: buffer("d_hidden", bytes(2 * max_batch * hidden)),
            outputs: buffer("outputs", bytes(2 * max_batch)),
            partials: buffer(
                "partial sums",
                bytes(max_batch.div_ceil(CHUNK) * (3 * hidden + 1)),
            ),
            entries: buffer("entries", bytes(2 * max_batch * MAX_ACTIVE)),
            segments: buffer("segments", bytes(2 * max_segments(max_batch))),
            first_segment: buffer("first segments", bytes(INPUTS + 1)),
            segment_sums: buffer("segment sums", bytes(max_segments(max_batch) * hidden)),
            total: buffer("total loss", bytes(1)),
        };
        let b = &buffers;
        let kernel = |name: &str, source: &str, bound: &[&wgpu::Buffer]| -> Kernel {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(Cow::Owned(format!("{COMMON}\n{source}"))),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            let entries: Vec<wgpu::BindGroupEntry<'_>> = std::iter::once(&b.sizes)
                .chain(bound.iter().copied())
                .enumerate()
                .map(|(i, buffer)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
            Kernel {
                pipeline,
                bind_group,
            }
        };
        let kernels = Kernels {
            forward_hidden: kernel(
                "forward_hidden",
                include_str!("gpu/forward_hidden.wgsl"),
                &[&b.params, &b.inputs, &b.hidden],
            ),
            forward_output: kernel(
                "forward_output",
                include_str!("gpu/forward_output.wgsl"),
                &[&b.params, &b.hidden, &b.targets, &b.outputs],
            ),
            backward_hidden: kernel(
                "backward_hidden",
                include_str!("gpu/backward_hidden.wgsl"),
                &[&b.params, &b.hidden, &b.outputs, &b.d_hidden],
            ),
            output_partials: kernel(
                "output_partials",
                include_str!("gpu/output_partials.wgsl"),
                &[&b.hidden, &b.outputs, &b.partials],
            ),
            bias_partials: kernel(
                "bias_partials",
                include_str!("gpu/bias_partials.wgsl"),
                &[&b.d_hidden, &b.partials],
            ),
            sum_partials: kernel(
                "sum_partials",
                include_str!("gpu/sum_partials.wgsl"),
                &[&b.partials, &b.grads],
            ),
            input_partials: kernel(
                "input_partials",
                include_str!("gpu/input_partials.wgsl"),
                &[&b.d_hidden, &b.entries, &b.segments, &b.segment_sums],
            ),
            input_gradients: kernel(
                "input_gradients",
                include_str!("gpu/input_gradients.wgsl"),
                &[&b.segment_sums, &b.first_segment, &b.grads],
            ),
            adam: kernel(
                "adam",
                include_str!("gpu/adam.wgsl"),
                &[&b.params, &b.grads, &b.first, &b.second],
            ),
            sum_losses: kernel(
                "sum_losses",
                include_str!("gpu/sum_losses.wgsl"),
                &[&b.outputs, &b.total],
            ),
        };

        let Checkpoint { net, adam, .. } = checkpoint;
        let gpu = GpuBackend {
            adapter: adapter.get_info().name,
            hidden,
            activation: net.activation,
            eval_scale: net.eval_scale,
            targets: Targets {
                eval_scale: net.eval_scale as f32,
                result_weight: settings.result_weight,
            },
            weight_decay: adam.weight_decay,
            steps: adam.steps,
            max_batch,
            threads: settings.threads.max(1),
            device,
            queue,
            buffers,
            kernels,
        };
        let upload = |buffer: &wgpu::Buffer, g: &Gradients| {
            let values = pack(
                &g.input_weights,
                &g.input_biases,
                &g.output_weights,
                g.output_bias,
            );
            gpu.queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(&values));
        };
        let params = pack(
            &net.input_weights,
            &net.input_biases,
            &net.output_weights,
            net.output_bias,
        );
        gpu.queue
            .write_buffer(&gpu.buffers.params, 0, bytemuck::cast_slice(&params));
        upload(&gpu.buffers.first, &adam.first);
        upload(&gpu.buffers.second, &adam.second);
        gpu.queue
            .write_buffer(&gpu.buffers.total, 0, bytemuck::cast_slice(&[0f32]));
        Ok(gpu)
    }

    /// The GPU's name, as the driver gives it.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Sends a batch's inputs and targets to the GPU, with its rows grouped by input and cut
    /// into segments for `input_partials`. Returns the number of segments.
    fn upload_batch(&self, data: &Dataset, batch: &[u32]) -> usize {
        assert!(
            batch.len() <= self.max_batch,
            "a batch larger than the GPU's buffers"
        );
        let batch = prepare(data, batch, self.targets, self.threads);
        let (queue, buffers) = (&self.queue, &self.buffers);
        queue.write_buffer(&buffers.inputs, 0, bytemuck::cast_slice(&batch.inputs));
        queue.write_buffer(&buffers.targets, 0, bytemuck::cast_slice(&batch.targets));
        if !batch.entries.is_empty() {
            queue.write_buffer(&buffers.entries, 0, bytemuck::cast_slice(&batch.entries));
            queue.write_buffer(&buffers.segments, 0, bytemuck::cast_slice(&batch.segments));
        }
        let first_segment = bytemuck::cast_slice(&batch.first_segment);
        queue.write_buffer(&buffers.first_segment, 0, first_segment);
        batch.segments.len() / 2
    }

    /// Sets the `Sizes` uniform for a batch of `batch` cut into `segments` segments, at
    /// `learning_rate`, with AdamW's bias corrections for the current step (computed as the
    /// CPU's are).
    fn write_sizes(&self, batch: usize, segments: usize, learning_rate: f32) {
        let correction = |beta: f32| (1.0 - f64::from(beta).powf(self.steps as f64)) as f32;
        let activation = match self.activation {
            Activation::ClippedRelu => 0,
            Activation::SquaredClippedRelu => 1,
        };
        let words: [u32; SIZES_WORDS] = [
            self.hidden as u32,
            batch as u32,
            activation,
            parameter_count(self.hidden) as u32,
            (1.0 / batch as f32).to_bits(),
            learning_rate.to_bits(),
            self.weight_decay.to_bits(),
            correction(AdamW::BETA1).to_bits(),
            correction(AdamW::BETA2).to_bits(),
            WEIGHT_LIMIT.to_bits(),
            segments as u32,
            0,
        ];
        self.queue
            .write_buffer(&self.buffers.sizes, 0, bytemuck::cast_slice(&words));
    }

    // ANCHOR: run
    /// Runs the kernels for a batch of `batch` already uploaded, cut into `segments` segments:
    /// the forward and backward passes, then AdamW's step if `step` is set, and adds the losses
    /// to the total.
    fn run(&self, batch: usize, segments: usize, step: bool) {
        let groups = |n: usize| (n as u32).div_ceil(WORKGROUP);
        let (h, rows) = (self.hidden, 2 * batch as u32);
        let k = &self.kernels;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("training step"),
                timestamp_writes: None,
            });
            let mut dispatch = |kernel: &Kernel, x: u32, y: u32| {
                pass.set_pipeline(&kernel.pipeline);
                pass.set_bind_group(0, &kernel.bind_group, &[]);
                pass.dispatch_workgroups(x, y, 1);
            };
            dispatch(&k.forward_hidden, groups(h), rows);
            dispatch(&k.forward_output, batch as u32, 1);
            dispatch(&k.backward_hidden, groups(h), rows);
            let chunks = batch.div_ceil(CHUNK) as u32;
            dispatch(&k.output_partials, groups(2 * h + 1), chunks);
            dispatch(&k.bias_partials, groups(h), chunks);
            dispatch(&k.sum_partials, groups(3 * h + 1), 1);
            dispatch(&k.input_partials, groups(h), segments as u32);
            dispatch(&k.input_gradients, groups(h), INPUTS as u32);
            if step {
                dispatch(&k.adam, groups(parameter_count(h)), 1);
            }
            dispatch(&k.sum_losses, 1, 1);
        }
        self.queue.submit([encoder.finish()]);
    }
    // ANCHOR_END: run

    /// Copies `n` numbers of `buffer` back from the GPU, waiting for everything before.
    fn read(&self, buffer: &wgpu::Buffer, n: usize) -> Vec<f32> {
        let size = bytes(n);
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read back"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        staging.map_async(wgpu::MapMode::Read, .., |result| {
            result.expect("the GPU couldn't map a buffer to read it back")
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU stopped answering");
        let values = {
            let view = staging
                .slice(..)
                .get_mapped_range()
                .expect("a buffer mapped for reading");
            view.chunks_exact(4)
                .take(n)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        };
        staging.unmap();
        values
    }

    /// The gradients of the mean loss over the records `batch` of `data`, and their summed
    /// loss, without changing the network: to test the GPU against the CPU.
    pub fn batch_gradients(&mut self, data: &Dataset, batch: &[u32]) -> (Gradients, f64) {
        let segments = self.upload_batch(data, batch);
        self.write_sizes(batch.len(), segments, 0.0);
        self.run(batch.len(), segments, false);
        let grads = unpack(
            &self.read(&self.buffers.grads, parameter_count(self.hidden)),
            self.hidden,
        );
        (grads, self.take_loss())
    }
}

impl Backend for GpuBackend {
    fn step(&mut self, data: &Dataset, batch: &[u32], learning_rate: f32) {
        let segments = self.upload_batch(data, batch);
        self.steps += 1;
        self.write_sizes(batch.len(), segments, learning_rate);
        self.run(batch.len(), segments, true);
        // Let the GPU free what finished batches used, without waiting for the rest.
        let _ = self.device.poll(wgpu::PollType::Poll);
    }

    fn take_loss(&mut self) -> f64 {
        let total = self.read(&self.buffers.total, 1)[0];
        self.queue
            .write_buffer(&self.buffers.total, 0, bytemuck::cast_slice(&[0f32]));
        f64::from(total)
    }

    fn checkpoint(&mut self, epochs_done: u32) -> Checkpoint {
        let n = parameter_count(self.hidden);
        let params = unpack(&self.read(&self.buffers.params, n), self.hidden);
        let first = unpack(&self.read(&self.buffers.first, n), self.hidden);
        let second = unpack(&self.read(&self.buffers.second, n), self.hidden);
        let net = FloatNetwork {
            hidden: self.hidden,
            activation: self.activation,
            eval_scale: self.eval_scale,
            input_weights: params.input_weights,
            input_biases: params.input_biases,
            output_weights: params.output_weights,
            output_bias: params.output_bias,
        };
        let adam = AdamW {
            learning_rate: 0.0,
            weight_decay: self.weight_decay,
            steps: self.steps,
            first,
            second,
        };
        Checkpoint {
            net,
            adam,
            epochs_done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backprop::{Scratch, backward, forward, loss};
    use crate::test_support::{random_network, random_records, teacher_records};
    use crate::train::{CpuBackend, fresh, train_with};

    fn settings() -> Settings {
        Settings {
            epochs: 1,
            batch_size: 256,
            learning_rate: 0.01,
            lr_drop_epoch: 100,
            weight_decay: 0.01,
            result_weight: 0.25,
            threads: 2,
            seed: 1,
        }
    }

    /// A GPU backend, or `None` (with a note) on a machine without a GPU.
    fn gpu(checkpoint: Checkpoint, settings: &Settings) -> Option<GpuBackend> {
        match GpuBackend::new(checkpoint, settings) {
            Ok(gpu) => Some(gpu),
            Err(e) => {
                eprintln!("skipping a GPU test: {e}");
                None
            }
        }
    }

    fn checkpoint(net: FloatNetwork, settings: &Settings) -> Checkpoint {
        Checkpoint {
            adam: AdamW::new(net.hidden, settings.learning_rate, settings.weight_decay),
            net,
            epochs_done: 0,
        }
    }

    /// The largest difference between two sets of numbers, and the largest of the first set.
    fn largest_difference(a: &Gradients, b: &Gradients) -> (f32, f32) {
        let pairs = |g: &Gradients| {
            let mut all = pack(
                &g.input_weights,
                &g.input_biases,
                &g.output_weights,
                g.output_bias,
            );
            all.shrink_to_fit();
            all
        };
        let (a, b) = (pairs(a), pairs(b));
        let difference = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max);
        (difference, a.iter().map(|x| x.abs()).fold(0.0, f32::max))
    }

    #[test]
    fn the_gpus_gradients_match_the_cpus() {
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            let net = random_network(64, activation, 5);
            let data = Dataset::from_records(&random_records(6, 20));
            let batch: Vec<u32> = (0..data.len() as u32).collect();
            let settings = Settings {
                batch_size: batch.len(),
                ..settings()
            };
            // The CPU's gradients of the mean loss.
            let targets = Targets {
                eval_scale: 146.0,
                result_weight: settings.result_weight,
            };
            let mut cpu = Gradients::zeros(64);
            let mut scratch = Scratch::new(64);
            let mut cpu_loss = 0.0;
            let mut rows_per_input = vec![0; INPUTS];
            for &i in &batch {
                let example = Example::from_record(&data.record(i as usize), targets);
                let (l, d) = loss(forward(&net, &example, &mut scratch), example.target);
                backward(&net, &example, &mut scratch, d, &mut cpu);
                cpu_loss += f64::from(l);
                for view in 0..2 {
                    for &input in example.inputs(view) {
                        rows_per_input[usize::from(input)] += 1;
                    }
                }
            }
            cpu.scale(1.0 / batch.len() as f32);
            // The commonest inputs' lists are long enough to be cut into several segments.
            assert!(rows_per_input.iter().max() > Some(&(2 * SEGMENT)));
            let Some(mut gpu) = gpu(checkpoint(net, &settings), &settings) else {
                return;
            };
            let (gpu_grads, gpu_loss) = gpu.batch_gradients(&data, &batch);
            let (difference, largest) = largest_difference(&cpu, &gpu_grads);
            assert!(
                difference <= 1e-4 * largest,
                "{activation:?}: {difference} against {largest}"
            );
            assert!(
                (cpu_loss - gpu_loss).abs() <= 1e-4 * cpu_loss,
                "{cpu_loss} vs {gpu_loss}"
            );
        }
    }

    #[test]
    fn an_adamw_step_on_the_gpu_matches_the_cpus() {
        let net = random_network(64, Activation::SquaredClippedRelu, 7);
        let data = Dataset::from_records(&random_records(8, 3));
        let batch: Vec<u32> = (0..data.len() as u32).collect();
        let settings = Settings {
            batch_size: batch.len(),
            ..settings()
        };
        let mut cpu = CpuBackend::new(checkpoint(net.clone(), &settings), &settings);
        let Some(mut gpu) = gpu(checkpoint(net, &settings), &settings) else {
            return;
        };
        for _ in 0..3 {
            cpu.step(&data, &batch, settings.learning_rate);
            gpu.step(&data, &batch, settings.learning_rate);
        }
        let (cpu, gpu) = (cpu.checkpoint(1), gpu.checkpoint(1));
        // Adam steps by about the learning rate whatever a gradient's size, so a weight whose
        // gradient is a rounding error away from zero could step either way: compare the rest.
        let g = |c: &Checkpoint| {
            pack(
                &c.adam.first.input_weights,
                &c.adam.first.input_biases,
                &c.adam.first.output_weights,
                c.adam.first.output_bias,
            )
        };
        let w = |c: &Checkpoint| {
            pack(
                &c.net.input_weights,
                &c.net.input_biases,
                &c.net.output_weights,
                c.net.output_bias,
            )
        };
        let (moments, (cpu_w, gpu_w)) = (g(&cpu), (w(&cpu), w(&gpu)));
        let mut compared = 0;
        for k in 0..cpu_w.len() {
            if moments[k].abs() > 1e-6 {
                assert!(
                    (cpu_w[k] - gpu_w[k]).abs() <= 1e-4,
                    "parameter {k}: {} vs {}",
                    cpu_w[k],
                    gpu_w[k]
                );
                compared += 1;
            }
        }
        assert!(compared > 1000, "only {compared} parameters moved");
        assert_eq!(gpu.adam.steps, 3);
    }

    #[test]
    fn training_on_the_gpu_learns_as_the_cpu_does() {
        let teacher = random_network(32, Activation::SquaredClippedRelu, 9);
        let data = Dataset::from_records(&teacher_records(&teacher, 10, 20));
        let mut settings = settings();
        settings.epochs = 12;
        settings.batch_size = 64;
        settings.result_weight = 0.0;
        settings.weight_decay = 0.0;
        let run = |backend: &mut dyn Backend| {
            let mut losses = Vec::new();
            train_with(
                &data,
                &Dataset::default(),
                &settings,
                backend,
                0,
                &mut |report| {
                    if let crate::train::Report::Epoch { train_loss, .. } = report {
                        losses.push(train_loss);
                    }
                },
            );
            losses
        };
        let start = fresh(32, Activation::SquaredClippedRelu, 146, &settings);
        let cpu = run(&mut CpuBackend::new(start.clone(), &settings));
        let Some(mut gpu) = gpu(start, &settings) else {
            return;
        };
        let gpu = run(&mut gpu);
        assert!(gpu[11] < 0.2 * gpu[0], "GPU losses {gpu:?}");
        // Rounding makes the runs drift apart a little, not differently.
        assert!(
            (gpu[11] - cpu[11]).abs() <= 0.25 * cpu[11],
            "GPU {gpu:?}, CPU {cpu:?}"
        );
    }

    #[test]
    fn any_number_of_threads_prepares_the_same_batch() {
        let data = Dataset::from_records(&random_records(3, 20));
        // Every other record, last first, as a shuffle would mix them.
        let batch: Vec<u32> = (0..data.len() as u32).rev().step_by(2).collect();
        let targets = Targets {
            eval_scale: 146.0,
            result_weight: 0.25,
        };
        let one = prepare(&data, &batch, targets, 1);
        for threads in [2, 3, 8, batch.len() + 5] {
            assert_eq!(
                prepare(&data, &batch, targets, threads),
                one,
                "{threads} threads"
            );
        }
        // The rows and targets are the examples', in order.
        for (e, &i) in batch.iter().enumerate() {
            let example = Example::from_record(&data.record(i as usize), targets);
            assert_eq!(one.targets[e], example.target);
            for view in 0..2 {
                let row = &one.inputs[(2 * e + view) * MAX_ACTIVE..][..MAX_ACTIVE];
                let on: Vec<u32> = example.inputs(view).iter().map(|&n| u32::from(n)).collect();
                assert_eq!(&row[..on.len()], on.as_slice());
                assert!(row[on.len()..].iter().all(|&n| n == NONE));
            }
        }
        // Each input's segments hold exactly the rows it is on in, in order.
        let mut cut = 0;
        for input in 0..INPUTS {
            let mut rows = Vec::new();
            let own = one.first_segment[input]..one.first_segment[input + 1];
            cut += usize::from(own.len() > 1);
            for s in own.map(|s| s as usize) {
                let (start, end) = (one.segments[2 * s], one.segments[2 * s + 1]);
                assert!(start < end && end - start <= SEGMENT as u32);
                rows.extend_from_slice(&one.entries[start as usize..end as usize]);
            }
            let expected: Vec<u32> = (0..2 * batch.len())
                .filter(|&row| {
                    one.inputs[row * MAX_ACTIVE..][..MAX_ACTIVE].contains(&(input as u32))
                })
                .map(|row| row as u32)
                .collect();
            assert_eq!(rows, expected, "input {input}");
        }
        assert!(cut > 0, "no input's list was long enough to be cut");
    }
}
