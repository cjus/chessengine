//! The training loop: epochs of shuffled batches, on all the CPU's cores.
//!
//! Each **epoch** visits every training record once, in a new random order (shuffled with the
//! seed plus the epoch's number, so a resumed run visits them in the same order). The records
//! are taken in **batches**: each batch's gradients are averaged and AdamW takes one step. A
//! batch is shared out among the threads, each summing gradients for its share into its own
//! buffer; the buffers are then added up in a fixed order, so the same settings and number of
//! threads always give the same network.
//!
//! The learning rate drops to a tenth for the last quarter of the epochs (by default): large
//! steps find the right region quickly, small ones settle into it.
//!
//! After each epoch the loss is measured on the validation set, which training never sees. The
//! training loss keeps falling as the network learns; if the validation loss stops falling, the
//! network has started to learn the training positions themselves rather than chess.

use std::thread;
use std::time::Instant;

use cb2026_nnue::{Activation, FloatNetwork};

use crate::adam::AdamW;
use crate::backprop::{Gradients, Scratch, backward, forward, loss};
use crate::checkpoint::Checkpoint;
use crate::data::Dataset;
use crate::example::{Example, MAX_ACTIVE, Targets};

/// What the learning rate is multiplied by from [`Settings::lr_drop_epoch`] on.
pub const LR_DROP: f32 = 0.1;
/// Progress is reported this many times an epoch.
const REPORTS_PER_EPOCH: usize = 20;

/// How to train.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Epochs in all, counting any already done.
    pub epochs: u32,
    /// Records per batch, so per AdamW step.
    pub batch_size: usize,
    /// AdamW's learning rate at the start.
    pub learning_rate: f32,
    /// The first epoch (counting from 0) trained at the lower learning rate.
    pub lr_drop_epoch: u32,
    /// AdamW's weight decay.
    pub weight_decay: f32,
    /// How much of each target is the game's result rather than the search's score.
    pub result_weight: f32,
    /// Threads to share each batch among.
    pub threads: usize,
    /// Seed for the starting weights and the shuffling.
    pub seed: u64,
}

/// What [`train`] reports as it goes.
#[derive(Debug)]
pub enum Report<'a> {
    /// Part of an epoch is done.
    Progress {
        /// The epoch, counting from 1.
        epoch: u32,
        /// Records trained on so far this epoch.
        done: usize,
        /// Records in the epoch.
        of: usize,
        /// The mean training loss so far this epoch.
        loss: f64,
    },
    /// An epoch is done.
    Epoch {
        /// The epoch, counting from 1.
        epoch: u32,
        /// The mean training loss over the epoch.
        train_loss: f64,
        /// The mean loss on the validation set after the epoch, if there is one.
        validation_loss: Option<f64>,
        /// Records trained on per second.
        records_per_second: f64,
        /// Training as it stands, to save.
        checkpoint: &'a Checkpoint,
    },
}

/// A small seeded pseudo-random generator (xorshift64*): starting weights and shuffles are the
/// same on every run with the same seed.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number in `0..n`.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// A number between `-limit` and `limit`, evenly spread.
    pub(crate) fn spread(&mut self, limit: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        (2.0 * unit - 1.0) * limit
    }
}

// ANCHOR: initial_network
/// Small random starting weights. First-layer weights are spread evenly within ±1/√32, so with
/// 32 inputs on a hidden value starts with a spread of about 0.6, partly inside the activation's
/// 0..1; output weights within ±1/√(2 × hidden), so the output starts small; biases at zero.
pub fn initial_network(
    hidden: usize,
    activation: Activation,
    eval_scale: u32,
    seed: u64,
) -> FloatNetwork {
    let mut rng = Rng::new(seed);
    let mut net = FloatNetwork::zeros(hidden, activation, eval_scale);
    let input_limit = 1.0 / (MAX_ACTIVE as f32).sqrt();
    let output_limit = 1.0 / ((2 * hidden) as f32).sqrt();
    net.input_weights
        .iter_mut()
        .for_each(|w| *w = rng.spread(input_limit));
    net.output_weights
        .iter_mut()
        .for_each(|w| *w = rng.spread(output_limit));
    net
}
// ANCHOR_END: initial_network

/// Training at its start: random weights and a fresh optimiser.
pub fn fresh(
    hidden: usize,
    activation: Activation,
    eval_scale: u32,
    settings: &Settings,
) -> Checkpoint {
    Checkpoint {
        net: initial_network(hidden, activation, eval_scale, settings.seed),
        adam: AdamW::new(hidden, settings.learning_rate, settings.weight_decay),
        epochs_done: 0,
    }
}

// ANCHOR: train
/// Trains from `checkpoint` until `settings.epochs` epochs are done, calling `report` as it
/// goes, and returns where training ended.
pub fn train(
    data: &Dataset,
    validation: &Dataset,
    settings: &Settings,
    mut checkpoint: Checkpoint,
    report: &mut dyn FnMut(Report<'_>),
) -> Checkpoint {
    let targets = Targets {
        eval_scale: checkpoint.net.eval_scale as f32,
        result_weight: settings.result_weight,
    };
    let threads = settings.threads.max(1);
    let mut workers: Vec<Worker> = (0..threads)
        .map(|_| Worker::new(checkpoint.net.hidden))
        .collect();
    for epoch in checkpoint.epochs_done..settings.epochs {
        let drop = if epoch >= settings.lr_drop_epoch {
            LR_DROP
        } else {
            1.0
        };
        checkpoint.adam.learning_rate = settings.learning_rate * drop;
        let order = shuffled(data.len(), settings.seed.wrapping_add(u64::from(epoch)));
        let started = Instant::now();
        let batches = order.len().div_ceil(settings.batch_size);
        let every = (batches / REPORTS_PER_EPOCH).max(1);
        let mut total_loss = 0.0;
        for (b, batch) in order.chunks(settings.batch_size).enumerate() {
            total_loss += batch_gradients(&checkpoint.net, data, batch, targets, &mut workers);
            let grads = &mut workers[0].grads;
            grads.scale(1.0 / batch.len() as f32);
            checkpoint.adam.step(&mut checkpoint.net, grads);
            if (b + 1) % every == 0 && b + 1 < batches {
                let done = (b + 1) * settings.batch_size;
                report(Report::Progress {
                    epoch: epoch + 1,
                    done,
                    of: order.len(),
                    loss: total_loss / done as f64,
                });
            }
        }
        checkpoint.epochs_done = epoch + 1;
        let validation_loss = (!validation.is_empty())
            .then(|| mean_loss(&checkpoint.net, validation, targets, threads));
        report(Report::Epoch {
            epoch: epoch + 1,
            train_loss: total_loss / order.len().max(1) as f64,
            validation_loss,
            records_per_second: order.len() as f64 / started.elapsed().as_secs_f64(),
            checkpoint: &checkpoint,
        });
    }
    checkpoint
}
// ANCHOR_END: train

/// One thread's gradient buffer and working space.
struct Worker {
    grads: Gradients,
    scratch: Scratch,
}

impl Worker {
    fn new(hidden: usize) -> Worker {
        Worker {
            grads: Gradients::zeros(hidden),
            scratch: Scratch::new(hidden),
        }
    }
}

/// Shares `batch` among the workers' threads, leaves the batch's summed gradients in
/// `workers[0].grads`, and returns its summed loss.
fn batch_gradients(
    net: &FloatNetwork,
    data: &Dataset,
    batch: &[u32],
    targets: Targets,
    workers: &mut [Worker],
) -> f64 {
    let share = batch.len().div_ceil(workers.len());
    let losses: Vec<f64> = thread::scope(|scope| {
        let handles: Vec<_> = batch
            .chunks(share)
            .zip(workers.iter_mut())
            .map(|(chunk, worker)| {
                scope.spawn(move || {
                    worker.grads.clear();
                    let mut total = 0.0;
                    for &i in chunk {
                        let example = Example::from_record(&data.record(i as usize), targets);
                        let output = forward(net, &example, &mut worker.scratch);
                        let (l, d_output) = loss(output, example.target);
                        backward(
                            net,
                            &example,
                            &mut worker.scratch,
                            d_output,
                            &mut worker.grads,
                        );
                        total += f64::from(l);
                    }
                    total
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a training thread panicked"))
            .collect()
    });
    // Add the threads' gradients up in a fixed order, so runs are repeatable.
    let (first, rest) = workers.split_at_mut(1);
    for worker in &rest[..losses.len() - 1] {
        first[0].grads.add(&worker.grads);
    }
    losses.iter().sum()
}

/// The mean loss of `net` on `data`, on `threads` threads.
pub fn mean_loss(net: &FloatNetwork, data: &Dataset, targets: Targets, threads: usize) -> f64 {
    let share = data.len().div_ceil(threads.max(1)).max(1);
    let total: f64 = thread::scope(|scope| {
        let handles: Vec<_> = (0..data.len())
            .step_by(share)
            .map(|start| {
                scope.spawn(move || {
                    let mut scratch = Scratch::new(net.hidden);
                    (start..(start + share).min(data.len()))
                        .map(|i| {
                            let example = Example::from_record(&data.record(i), targets);
                            f64::from(loss(forward(net, &example, &mut scratch), example.target).0)
                        })
                        .sum::<f64>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a validation thread panicked"))
            .sum()
    });
    total / data.len().max(1) as f64
}

/// The numbers `0..n` in a random order (Fisher–Yates), the same for the same seed.
fn shuffled(n: usize, seed: u64) -> Vec<u32> {
    assert!(n <= u32::MAX as usize, "too many records to shuffle");
    let mut order: Vec<u32> = (0..n as u32).collect();
    let mut rng = Rng::new(seed);
    for i in (1..n).rev() {
        order.swap(i, rng.below(i + 1));
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{random_network, teacher_records};

    fn settings(threads: usize) -> Settings {
        Settings {
            epochs: 30,
            batch_size: 32,
            learning_rate: 0.01,
            lr_drop_epoch: 24,
            weight_decay: 0.0,
            result_weight: 0.0,
            threads,
            seed: 1,
        }
    }

    #[test]
    fn shuffles_are_permutations_and_repeatable() {
        let a = shuffled(1000, 5);
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..1000).collect::<Vec<u32>>());
        assert_eq!(a, shuffled(1000, 5));
        assert_ne!(a, shuffled(1000, 6));
    }

    #[test]
    fn training_learns_what_a_teacher_network_knows() {
        // The scores come from a random "teacher" network, which a student of the same shape
        // could match exactly. 40 random games give 1,920 positions to train on and 480 to
        // validate on.
        let teacher = random_network(32, Activation::SquaredClippedRelu, 41);
        let mut data = Dataset::from_records(&teacher_records(&teacher, 42, 40));
        let validation = data.split_off_validation(0.2);
        let settings = settings(2);
        let targets = Targets {
            eval_scale: 146.0,
            result_weight: 0.0,
        };
        let start = fresh(32, Activation::SquaredClippedRelu, 146, &settings);
        let before = mean_loss(&start.net, &data, targets, 2);
        let end = train(&data, &validation, &settings, start, &mut |_| {});
        let after = mean_loss(&end.net, &data, targets, 2);
        // The positions it trained on: learnt almost exactly (measured 0.0040 -> 0.000022).
        assert!(after < 0.05 * before, "training loss {before} -> {after}");
        // Positions it never saw: better than always guessing the average target, but far from
        // exact, since 1,920 positions are few for 25,000 weights. It overfits (measured 0.00126
        // against 0.00204 for the guess; with 12 games it did no better than the guess).
        let examples = |d: &Dataset| -> Vec<Example> {
            (0..d.len())
                .map(|i| Example::from_record(&d.record(i), targets))
                .collect()
        };
        let mean_target = examples(&data)
            .iter()
            .map(|e| f64::from(e.target))
            .sum::<f64>()
            / data.len() as f64;
        let guess = examples(&validation)
            .iter()
            .map(|e| (f64::from(e.target) - mean_target).powi(2))
            .sum::<f64>()
            / validation.len() as f64;
        let unseen = mean_loss(&end.net, &validation, targets, 2);
        assert!(
            unseen < 0.8 * guess,
            "validation loss {unseen}, guessing {guess}"
        );
    }

    #[test]
    fn resuming_from_a_checkpoint_gives_the_same_network() {
        let teacher = random_network(32, Activation::ClippedRelu, 43);
        let data = Dataset::from_records(&teacher_records(&teacher, 44, 4));
        let mut all = settings(2);
        all.epochs = 4;
        let start = fresh(32, Activation::ClippedRelu, 146, &all);
        let straight = train(&data, &Dataset::default(), &all, start.clone(), &mut |_| {});
        let mut half = all.clone();
        half.epochs = 2;
        let saved = train(&data, &Dataset::default(), &half, start, &mut |_| {}).to_bytes();
        let resumed = Checkpoint::from_bytes(&saved, all.learning_rate, all.weight_decay).unwrap();
        let resumed = train(&data, &Dataset::default(), &all, resumed, &mut |_| {});
        assert_eq!(resumed, straight);
    }

    #[test]
    fn a_trained_network_exports_to_a_file_the_engine_can_read() {
        let teacher = random_network(32, Activation::SquaredClippedRelu, 47);
        let records = teacher_records(&teacher, 48, 4);
        let data = Dataset::from_records(&records);
        let mut s = settings(2);
        s.epochs = 3;
        let start = fresh(32, Activation::SquaredClippedRelu, 146, &s);
        let float = train(&data, &Dataset::default(), &s, start, &mut |_| {}).net;
        let bytes = float.quantize().unwrap().to_bytes();
        let net = cb2026_nnue::Network::from_bytes(&bytes).unwrap();
        for record in records.iter().step_by(7) {
            let pos = record.to_position();
            let (integer, exact) = (net.evaluate_position(&pos) as f32, float.evaluate(&pos));
            assert!((integer - exact).abs() < 10.0, "{integer} vs {exact}");
        }
    }

    #[test]
    fn more_threads_give_nearly_the_same_network() {
        // The batch is split differently, so sums are added in another order: tiny differences.
        let teacher = random_network(32, Activation::SquaredClippedRelu, 45);
        let data = Dataset::from_records(&teacher_records(&teacher, 46, 4));
        let run = |threads| {
            let mut s = settings(threads);
            s.epochs = 2;
            let start = fresh(32, Activation::SquaredClippedRelu, 146, &s);
            train(&data, &Dataset::default(), &s, start, &mut |_| {}).net
        };
        let (one, three) = (run(1), run(3));
        let largest = one
            .input_weights
            .iter()
            .zip(&three.input_weights)
            .chain(one.output_weights.iter().zip(&three.output_weights))
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(largest < 1e-4, "largest difference {largest}");
    }
}
