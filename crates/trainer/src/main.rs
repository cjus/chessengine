//! Trains an NNUE network on self-play records.
//!
//! ```text
//! cargo run --release -p cb2026-trainer -- --data FILE [--data FILE ...] --out DIR
//!     [--hidden 256] [--activation screlu|crelu] [--epochs 20] [--batch 16384]
//!     [--lr 0.001] [--weight-decay 0.01] [--result-weight 0.25] [--eval-scale N|fit]
//!     [--validation 0.01] [--threads N] [--seed 1] [--resume DIR/checkpoint.bin]
//! ```
//!
//! The records come from `datagen --format records`. The last `--validation` share of them is
//! held back to measure the network on games it never trained on. `--eval-scale fit` (the
//! default) fits the scale between centipawns and results from the training records.
//!
//! After each epoch it writes the network as `DIR/epoch-NN.nnue`, with whole-number weights
//! ready for the engine, and the training state as `DIR/checkpoint.bin`, from which `--resume`
//! continues (the network's shape and eval scale then come from the checkpoint). It prints the
//! training and validation losses and the speed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use cb2026_nnue::{Activation, network::DEFAULT_HIDDEN};
use cb2026_trainer::checkpoint::Checkpoint;
use cb2026_trainer::data::Dataset;
use cb2026_trainer::scale::fit_eval_scale;
use cb2026_trainer::train::{Report, Settings, fresh, train};

const USAGE: &str = "Usage: trainer --data FILE [--data FILE ...] --out DIR [--hidden N] \
[--activation screlu|crelu] [--epochs N] [--batch N] [--lr R] [--weight-decay R] \
[--result-weight W] [--eval-scale N|fit] [--validation F] [--threads N] [--seed N] \
[--resume CHECKPOINT]";

/// Epochs, unless `--epochs` says otherwise.
const DEFAULT_EPOCHS: u32 = 20;
/// Records per batch.
const DEFAULT_BATCH: usize = 16_384;
/// AdamW's starting learning rate.
const DEFAULT_LEARNING_RATE: f32 = 0.001;
/// AdamW's weight decay.
const DEFAULT_WEIGHT_DECAY: f32 = 0.01;
/// The share of each target that is the game's result.
const DEFAULT_RESULT_WEIGHT: f32 = 0.25;
/// The share of the records held back for validation.
const DEFAULT_VALIDATION: f64 = 0.01;
/// The learning rate drops for the epochs from this share of the run on.
const LR_DROP_AT: f64 = 0.75;
/// The eval scale is fitted on at most this many records, spread evenly through the data.
const FIT_SAMPLE: usize = 1_000_000;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("trainer: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut data_files = Vec::new();
    let mut out = None;
    let mut hidden = DEFAULT_HIDDEN;
    let mut activation = Activation::SquaredClippedRelu;
    let mut eval_scale = None;
    let mut validation_share = DEFAULT_VALIDATION;
    let mut resume = None;
    let mut settings = Settings {
        epochs: DEFAULT_EPOCHS,
        batch_size: DEFAULT_BATCH,
        learning_rate: DEFAULT_LEARNING_RATE,
        lr_drop_epoch: 0,
        weight_decay: DEFAULT_WEIGHT_DECAY,
        result_weight: DEFAULT_RESULT_WEIGHT,
        threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        seed: 1,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value\n{USAGE}"));
        fn parse<T: std::str::FromStr>(arg: &str, text: String) -> Result<T, String>
        where
            T::Err: std::fmt::Display,
        {
            text.parse().map_err(|e| format!("{arg}: {e}"))
        }
        match arg.as_str() {
            "--data" => data_files.push(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--hidden" => hidden = parse(&arg, value()?)?,
            "--activation" => {
                activation = match value()?.as_str() {
                    "screlu" => Activation::SquaredClippedRelu,
                    "crelu" => Activation::ClippedRelu,
                    other => return Err(format!("unknown activation '{other}' (screlu or crelu)")),
                }
            }
            "--epochs" => settings.epochs = parse(&arg, value()?)?,
            "--batch" => settings.batch_size = parse::<usize>(&arg, value()?)?.max(1),
            "--lr" => settings.learning_rate = parse(&arg, value()?)?,
            "--weight-decay" => settings.weight_decay = parse(&arg, value()?)?,
            "--result-weight" => settings.result_weight = parse(&arg, value()?)?,
            "--eval-scale" => {
                eval_scale = match value()?.as_str() {
                    "fit" => None,
                    text => Some(parse::<u32>(&arg, text.to_string())?),
                }
            }
            "--validation" => validation_share = parse(&arg, value()?)?,
            "--threads" => settings.threads = parse::<usize>(&arg, value()?)?.max(1),
            "--seed" => settings.seed = parse(&arg, value()?)?,
            "--resume" => resume = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument '{other}'\n{USAGE}")),
        }
    }
    if data_files.is_empty() {
        return Err(format!("--data is required\n{USAGE}"));
    }
    let out = out.ok_or(format!("--out is required\n{USAGE}"))?;
    if !(0.0..=1.0).contains(&settings.result_weight) {
        return Err("--result-weight must be between 0 and 1".to_string());
    }
    settings.lr_drop_epoch = (f64::from(settings.epochs) * LR_DROP_AT).round() as u32;

    // The records.
    let mut bytes = Vec::new();
    for file in &data_files {
        let mut contents =
            fs::read(file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
        bytes.append(&mut contents);
    }
    let mut data = Dataset::from_bytes(bytes)?;
    let validation = data.split_off_validation(validation_share);
    if data.is_empty() {
        return Err("no records to train on".to_string());
    }
    println!(
        "{} records from {} file(s): {} to train on, {} to validate on",
        data.len() + validation.len(),
        data_files.len(),
        data.len(),
        validation.len()
    );

    // Where training starts.
    let start = match &resume {
        Some(path) => {
            let saved =
                fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
            let checkpoint =
                Checkpoint::from_bytes(&saved, settings.learning_rate, settings.weight_decay)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
            println!(
                "resuming from {} after {} epochs",
                path.display(),
                checkpoint.epochs_done
            );
            checkpoint
        }
        None => {
            let scale = match eval_scale {
                Some(scale) => {
                    println!("eval scale {scale} (given)");
                    scale
                }
                None => {
                    let step = data.len().div_ceil(FIT_SAMPLE).max(1);
                    let pairs: Vec<(f32, f32)> = (0..data.len())
                        .step_by(step)
                        .map(|i| {
                            let record = data.record(i);
                            (f32::from(record.score()), record.result().for_white())
                        })
                        .collect();
                    let fitted = fit_eval_scale(&pairs);
                    println!("eval scale {fitted:.1} (fitted on {} records)", pairs.len());
                    fitted.round() as u32
                }
            };
            fresh(hidden, activation, scale, &settings)
        }
    };
    println!(
        "network 768 -> {} x 2 -> 1, {}, eval scale {}; {} epochs of batches of {}, {} threads",
        start.net.hidden,
        match start.net.activation {
            Activation::SquaredClippedRelu => "SCReLU",
            Activation::ClippedRelu => "CReLU",
        },
        start.net.eval_scale,
        settings.epochs,
        settings.batch_size,
        settings.threads
    );

    fs::create_dir_all(&out).map_err(|e| format!("can't create {}: {e}", out.display()))?;
    let began = Instant::now();
    let mut failure = None;
    train(
        &data,
        &validation,
        &settings,
        start,
        &mut |report| match report {
            Report::Progress {
                epoch,
                done,
                of,
                loss,
            } => {
                println!(
                    "  epoch {epoch}: {:.0}%, loss {loss:.6}",
                    100.0 * done as f64 / of as f64
                );
            }
            Report::Epoch {
                epoch,
                train_loss,
                validation_loss,
                records_per_second,
                checkpoint,
            } => {
                let validation =
                    validation_loss.map_or(String::new(), |v| format!(", validation {v:.6}"));
                println!(
                    "epoch {epoch}/{}: training loss {train_loss:.6}{validation}, {:.0} records/s, {:.0} s in all",
                    settings.epochs,
                    records_per_second,
                    began.elapsed().as_secs_f64()
                );
                let saved = checkpoint
                    .net
                    .quantize()
                    .map_err(|e| e.to_string())
                    .and_then(|net| {
                        write(&out.join(format!("epoch-{epoch:02}.nnue")), &net.to_bytes())
                    })
                    .and_then(|()| write(&out.join("checkpoint.bin"), &checkpoint.to_bytes()));
                if let Err(e) = saved {
                    failure.get_or_insert(e);
                }
            }
        },
    );
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Writes `bytes` to `path` through a temporary file, so a crash never leaves half a file.
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("part");
    fs::write(&temporary, bytes)
        .and_then(|()| fs::rename(&temporary, path))
        .map_err(|e| format!("can't write {}: {e}", path.display()))
}
