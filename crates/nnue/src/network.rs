//! The network the engine runs: whole-number weights, the accumulator, and the file format.
//!
//! ## Whole numbers instead of floats
//!
//! The trainer learns floating-point weights ([`FloatNetwork`]). The engine uses them multiplied
//! by a fixed scale and rounded ("quantised"), because integer arithmetic is exact and the CPU
//! does many more small integer operations at once than float ones:
//!
//! | Numbers | Stored as | Scale |
//! | --- | --- | --- |
//! | First-layer weights and biases | `i16` | [`QA`] = 255 |
//! | Output weights | `i16` | [`QB`] = 64 |
//! | Output bias | `i32` | `QA × QB` |
//!
//! A hidden value of 1.0 is 255, so clipping a hidden value to 0..1 is clamping it to 0..255.
//!
//! ## Why it can't overflow
//!
//! The trainer keeps every weight and first-layer bias within ±[`WEIGHT_LIMIT`](crate::float::WEIGHT_LIMIT) = ±1.98, so a
//! first-layer number is at most 505 ([`MAX_INPUT_WEIGHT`]) and an output weight at most 127
//! ([`MAX_OUTPUT_WEIGHT`]); [`Network::from_bytes`] refuses a file that breaks those limits.
//! A legal position has at most 32 pieces, so a hidden value is at most 33 × 505 = 16,665, well
//! inside an `i16` (32,767). An activation times an output weight is at most 255 × 127 = 32,385,
//! which fits in an `i16` too (a faster version may rely on that), and the output is summed in an
//! `i64`. The accumulator adds with wrapping arithmetic, so a made-up position with more than 32
//! pieces gives a meaningless score rather than a crash.
//!
//! ## The output
//!
//! The output layer's sum, plus its bias, is a number at the scale `QA × QB`. Multiplied by the
//! network's **eval scale** and divided by `QA × QB`, it becomes centipawns, from the side to
//! move's point of view, like the hand-crafted evaluation's.
//!
//! The eval scale has to match the hand-crafted evaluation's centipawns. The trainer turns a
//! score into an expected result with a sigmoid, `σ(cp / scale)`, and fits the network both to
//! search scores and to game results. If the scale doesn't match how cb2026's centipawns relate
//! to results, the two targets disagree and the network's centipawns drift away from the
//! hand-crafted evaluation's, which changes the meaning of every pruning margin the search tuned
//! in centipawns. So the scale is fitted from the data, like the Texel tuner's `K` (chapter 16):
//! on 2026-10-23, `K` = 1.19 on 40,304 self-play positions, which is a scale of
//! 400 / (1.19 × ln 10) ≈ 146. Many engines use 400; cb2026's centipawns are steeper than that.
//!
//! ## The file
//!
//! A network file is a 48-byte header, then the numbers, all little-endian:
//!
//! | Bytes | Field |
//! | --- | --- |
//! | 0–7 | the magic `CB2026NN` |
//! | 8–11 | format version (1) |
//! | 12–15 | input set (0: the 768 inputs of [`features`]) |
//! | 16–19 | hidden layer size (a multiple of 32, at most 4096) |
//! | 20–23 | activation (0: CReLU, 1: SCReLU) |
//! | 24–27 | output buckets (1) |
//! | 28–31, 32–35 | the scales [`QA`] and [`QB`] (255 and 64) |
//! | 36–39 | eval scale (centipawns per unit of output) |
//! | 40–47 | checksum: FNV-1a (64-bit) of everything after the header |
//! | 48– | first-layer weights (768 rows of one `i16` per hidden value), first-layer biases, output weights (the side to move's half first), output bias (`i32`) |
//!
//! Each input's weights are one contiguous row, so switching an input on or off adds or
//! subtracts one row. The header describes the network fully, so a file can be checked before
//! it is used and shown by the shell. The checksum catches damaged or truncated files, not
//! deliberate changes.

use cb2026_chess::{Color, Position};

use crate::features::{self, Changes, INPUTS};
use crate::float::FloatNetwork;

// ANCHOR: scales
/// The scale of first-layer weights and biases: a hidden value of 1.0 is 255.
pub const QA: i32 = 255;
/// The scale of output weights.
pub const QB: i32 = 64;
/// The largest first-layer weight or bias: [`WEIGHT_LIMIT`](crate::float::WEIGHT_LIMIT) × [`QA`], rounded.
pub const MAX_INPUT_WEIGHT: i16 = 505;
/// The largest output weight: [`WEIGHT_LIMIT`](crate::float::WEIGHT_LIMIT) × [`QB`], rounded.
pub const MAX_OUTPUT_WEIGHT: i16 = 127;
// ANCHOR_END: scales

/// Hidden layer sizes must be a multiple of this, so vectorised loops have no leftovers.
pub const HIDDEN_MULTIPLE: usize = 32;
/// The largest hidden layer a file may ask for (a damaged file mustn't ask for gigabytes).
pub const MAX_HIDDEN: usize = 4096;
/// The hidden layer size of cb2026's first networks: about 200,000 weights, a good fit for the
/// 100 million positions a first network trains on.
pub const DEFAULT_HIDDEN: usize = 256;
/// The largest eval scale a file may give.
pub const MAX_EVAL_SCALE: u32 = 10_000;

const MAGIC: &[u8; 8] = b"CB2026NN";
const FORMAT_VERSION: u32 = 1;
/// The input set of [`features`]: 768 inputs, piece and square from each side's view.
const INPUT_SET_768: u32 = 0;
/// Networks have one output for now; later ones may pick one of several by material.
const OUTPUT_BUCKETS: u32 = 1;
const HEADER_LEN: usize = 48;

/// How a hidden value becomes the output layer's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// Clipped ReLU (CReLU): the value clamped to 0..1.
    ClippedRelu,
    /// Squared clipped ReLU (SCReLU): the value clamped to 0..1, then squared. It lets the network
    /// weigh large hidden values more, and usually makes a stronger network than CReLU.
    SquaredClippedRelu,
}

impl Activation {
    const fn code(self) -> u32 {
        match self {
            Activation::ClippedRelu => 0,
            Activation::SquaredClippedRelu => 1,
        }
    }

    const fn from_code(code: u32) -> Option<Activation> {
        match code {
            0 => Some(Activation::ClippedRelu),
            1 => Some(Activation::SquaredClippedRelu),
            _ => None,
        }
    }
}

/// Why a network couldn't be made or read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    /// The file is shorter than its header says it should be, or longer.
    WrongLength {
        /// The length the header implies.
        expected: usize,
        /// The length found.
        found: usize,
    },
    /// The file doesn't start with `CB2026NN`: it isn't a cb2026 network.
    BadMagic,
    /// The file is a format version this cb2026 doesn't know.
    UnsupportedVersion(u32),
    /// The network uses inputs this cb2026 doesn't know.
    UnsupportedInputs(u32),
    /// The hidden layer size isn't a multiple of 32 between 32 and 4096.
    BadHiddenSize(usize),
    /// The activation code isn't one this cb2026 knows.
    UnknownActivation(u32),
    /// The network has several output buckets, which this cb2026 doesn't support.
    UnsupportedOutputBuckets(u32),
    /// The file's scales aren't 255 and 64.
    WrongScales {
        /// The first-layer scale found.
        qa: u32,
        /// The output scale found.
        qb: u32,
    },
    /// The eval scale isn't between 1 and 10,000.
    BadEvalScale(u32),
    /// The numbers don't match the header's checksum: the file is damaged.
    BadChecksum,
    /// A weight is outside the limits that keep the arithmetic from overflowing.
    WeightOutOfRange,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::WrongLength { expected, found } => {
                write!(f, "expected {expected} bytes, found {found}")
            }
            NetError::BadMagic => write!(f, "not a cb2026 network (no CB2026NN header)"),
            NetError::UnsupportedVersion(v) => {
                write!(
                    f,
                    "network format version {v}; this cb2026 reads version {FORMAT_VERSION}"
                )
            }
            NetError::UnsupportedInputs(i) => write!(f, "unknown input set {i}"),
            NetError::BadHiddenSize(h) => write!(
                f,
                "hidden layer size {h}; it must be a multiple of {HIDDEN_MULTIPLE} up to {MAX_HIDDEN}"
            ),
            NetError::UnknownActivation(a) => write!(f, "unknown activation {a}"),
            NetError::UnsupportedOutputBuckets(b) => {
                write!(
                    f,
                    "{b} output buckets; this cb2026 supports {OUTPUT_BUCKETS}"
                )
            }
            NetError::WrongScales { qa, qb } => {
                write!(f, "scales {qa} and {qb}; this cb2026 uses {QA} and {QB}")
            }
            NetError::BadEvalScale(s) => {
                write!(
                    f,
                    "eval scale {s}; it must be between 1 and {MAX_EVAL_SCALE}"
                )
            }
            NetError::BadChecksum => write!(f, "the checksum doesn't match: the file is damaged"),
            NetError::WeightOutOfRange => write!(
                f,
                "a weight is outside ±{MAX_INPUT_WEIGHT} (first layer) or ±{MAX_OUTPUT_WEIGHT} (output)"
            ),
        }
    }
}

impl std::error::Error for NetError {}

/// A network with whole-number weights, as the engine runs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    hidden: usize,
    activation: Activation,
    eval_scale: u32,
    /// [`INPUTS`] rows of `hidden` weights, one row per input.
    input_weights: Vec<i16>,
    input_biases: Vec<i16>,
    /// `2 × hidden` weights: the side to move's half, then the other side's.
    output_weights: Vec<i16>,
    output_bias: i32,
}

/// The first layer's values for a position, from both sides' views: the **accumulator**. It is
/// computed once with [`Network::accumulator`] and then kept up to date move by move with
/// [`Network::update`], which is what makes the network cheap to evaluate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accumulator {
    /// One hidden layer per view, indexed by [`Color::index`].
    views: [Vec<i16>; 2],
}

impl Accumulator {
    /// The hidden layer seen from `view`'s side, before the activation.
    pub fn view(&self, view: Color) -> &[i16] {
        &self.views[view.index()]
    }
}

impl Network {
    /// A network from its parts, checking the sizes and limits that keep it safe to run.
    ///
    /// # Panics
    ///
    /// If a vector's length doesn't match `hidden`: that is a bug in the caller, not bad input.
    pub fn from_parts(
        hidden: usize,
        activation: Activation,
        eval_scale: u32,
        input_weights: Vec<i16>,
        input_biases: Vec<i16>,
        output_weights: Vec<i16>,
        output_bias: i32,
    ) -> Result<Network, NetError> {
        check_shape(hidden, eval_scale)?;
        assert_eq!(input_weights.len(), INPUTS * hidden, "input weights");
        assert_eq!(input_biases.len(), hidden, "input biases");
        assert_eq!(output_weights.len(), 2 * hidden, "output weights");
        let within = |values: &[i16], limit: i16| values.iter().all(|v| v.abs() <= limit);
        if !within(&input_weights, MAX_INPUT_WEIGHT)
            || !within(&input_biases, MAX_INPUT_WEIGHT)
            || !within(&output_weights, MAX_OUTPUT_WEIGHT)
        {
            return Err(NetError::WeightOutOfRange);
        }
        Ok(Network {
            hidden,
            activation,
            eval_scale,
            input_weights,
            input_biases,
            output_weights,
            output_bias,
        })
    }

    /// The hidden layer's size, per view.
    pub fn hidden(&self) -> usize {
        self.hidden
    }

    /// The network's activation.
    pub fn activation(&self) -> Activation {
        self.activation
    }

    /// Centipawns per unit of output (see "The output" above).
    pub fn eval_scale(&self) -> u32 {
        self.eval_scale
    }

    /// The checksum of the network's numbers, as stored in its file. It also names the network:
    /// two networks with the same checksum are almost certainly the same.
    pub fn checksum(&self) -> u64 {
        fnv1a(&self.payload())
    }

    /// The weights of one input: a row of one weight per hidden value.
    fn row(&self, input: usize) -> &[i16] {
        &self.input_weights[input * self.hidden..(input + 1) * self.hidden]
    }

    // ANCHOR: accumulator
    /// The accumulator for `pos`, computed from scratch: the biases plus the row of every input
    /// that is on.
    pub fn accumulator(&self, pos: &Position) -> Accumulator {
        let views = Color::ALL.map(|view| {
            let mut values = self.input_biases.clone();
            for input in features::active(pos, view) {
                add(&mut values, self.row(input));
            }
            values
        });
        Accumulator { views }
    }

    /// Brings `acc` up to date after a move with these `changes`: subtract the rows of the
    /// inputs switched off, add the rows of those switched on. Same result as
    /// [`Network::accumulator`] on the new position, for a fraction of the work.
    pub fn update(&self, acc: &mut Accumulator, changes: &Changes) {
        for view in Color::ALL {
            let values = &mut acc.views[view.index()];
            for (piece, square) in changes.removed() {
                subtract(values, self.row(features::index(view, piece, square)));
            }
            for (piece, square) in changes.added() {
                add(values, self.row(features::index(view, piece, square)));
            }
        }
    }
    // ANCHOR_END: accumulator

    // ANCHOR: evaluate
    /// The position's value in centipawns, from `side_to_move`'s point of view, given its
    /// accumulator. Nothing here keeps it below the search's mate scores: the engine clamps it.
    pub fn evaluate(&self, acc: &Accumulator, side_to_move: Color) -> i32 {
        let ours = acc.view(side_to_move);
        let theirs = acc.view(!side_to_move);
        let (our_weights, their_weights) = self.output_weights.split_at(self.hidden);
        // The sum is at the scale QA × QB. SCReLU's squares are at QA × QA × QB: divide one QA out.
        let sum = match self.activation {
            Activation::ClippedRelu => {
                crelu_dot(ours, our_weights) + crelu_dot(theirs, their_weights)
            }
            Activation::SquaredClippedRelu => {
                (screlu_dot(ours, our_weights) + screlu_dot(theirs, their_weights)) / i64::from(QA)
            }
        };
        let output = sum + i64::from(self.output_bias);
        // The sum is at most 2 × 4096 × 255 × 127 ≈ 2.7 × 10^8 and the bias an i32 (≈ 2.1 × 10^9);
        // times an eval scale of at most 10^4 that fits easily in an i64, and divided by
        // QA × QB it is at most about 1.5 × 10^9, inside an i32.
        (output * i64::from(self.eval_scale) / i64::from(QA * QB)) as i32
    }
    // ANCHOR_END: evaluate

    /// The value of `pos` in centipawns for the side to move, computing its accumulator from
    /// scratch. Handy for tests and tools; the search keeps accumulators instead.
    pub fn evaluate_position(&self, pos: &Position) -> i32 {
        self.evaluate(&self.accumulator(pos), pos.side_to_move())
    }

    /// The network's numbers in file order, little-endian: everything after the header.
    fn payload(&self) -> Vec<u8> {
        let numbers =
            self.input_weights.len() + self.input_biases.len() + self.output_weights.len();
        let mut bytes = Vec::with_capacity(2 * numbers + 4);
        for value in self
            .input_weights
            .iter()
            .chain(&self.input_biases)
            .chain(&self.output_weights)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.output_bias.to_le_bytes());
        bytes
    }

    /// The network as a file (see "The file" above).
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload = self.payload();
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
        bytes.extend_from_slice(MAGIC);
        let fields = [
            FORMAT_VERSION,
            INPUT_SET_768,
            self.hidden as u32,
            self.activation.code(),
            OUTPUT_BUCKETS,
            QA as u32,
            QB as u32,
            self.eval_scale,
        ];
        for field in fields {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        bytes.extend_from_slice(&fnv1a(&payload).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    /// Reads a network file, checking everything before trusting it.
    pub fn from_bytes(bytes: &[u8]) -> Result<Network, NetError> {
        if bytes.len() < HEADER_LEN {
            return Err(NetError::WrongLength {
                expected: HEADER_LEN,
                found: bytes.len(),
            });
        }
        if &bytes[..8] != MAGIC {
            return Err(NetError::BadMagic);
        }
        let field = |n: usize| {
            let at = 8 + 4 * n;
            u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
        };
        let version = field(0);
        if version != FORMAT_VERSION {
            return Err(NetError::UnsupportedVersion(version));
        }
        if field(1) != INPUT_SET_768 {
            return Err(NetError::UnsupportedInputs(field(1)));
        }
        let hidden = field(2) as usize;
        let activation =
            Activation::from_code(field(3)).ok_or(NetError::UnknownActivation(field(3)))?;
        if field(4) != OUTPUT_BUCKETS {
            return Err(NetError::UnsupportedOutputBuckets(field(4)));
        }
        let (qa, qb) = (field(5), field(6));
        if (qa, qb) != (QA as u32, QB as u32) {
            return Err(NetError::WrongScales { qa, qb });
        }
        let eval_scale = field(7);
        check_shape(hidden, eval_scale)?;

        let numbers = INPUTS * hidden + hidden + 2 * hidden;
        let expected = HEADER_LEN + 2 * numbers + 4;
        if bytes.len() != expected {
            return Err(NetError::WrongLength {
                expected,
                found: bytes.len(),
            });
        }
        let checksum = u64::from_le_bytes(bytes[40..48].try_into().expect("8 bytes"));
        let payload = &bytes[HEADER_LEN..];
        if fnv1a(payload) != checksum {
            return Err(NetError::BadChecksum);
        }

        let mut values = payload[..2 * numbers]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]));
        let mut take = |n: usize| values.by_ref().take(n).collect::<Vec<i16>>();
        let input_weights = take(INPUTS * hidden);
        let input_biases = take(hidden);
        let output_weights = take(2 * hidden);
        let output_bias = i32::from_le_bytes(payload[2 * numbers..].try_into().expect("4 bytes"));
        Network::from_parts(
            hidden,
            activation,
            eval_scale,
            input_weights,
            input_biases,
            output_weights,
            output_bias,
        )
    }

    /// The same network with floating-point weights, undoing the scales. Rounding is lost, so
    /// it is close to the network this one was made from, not equal.
    pub fn to_float(&self) -> FloatNetwork {
        let unscale = |values: &[i16], scale: i32| -> Vec<f32> {
            values
                .iter()
                .map(|&v| f32::from(v) / scale as f32)
                .collect()
        };
        FloatNetwork {
            hidden: self.hidden,
            activation: self.activation,
            eval_scale: self.eval_scale,
            input_weights: unscale(&self.input_weights, QA),
            input_biases: unscale(&self.input_biases, QA),
            output_weights: unscale(&self.output_weights, QB),
            output_bias: self.output_bias as f32 / (QA * QB) as f32,
        }
    }
}

/// Checks the hidden layer's size and the eval scale.
fn check_shape(hidden: usize, eval_scale: u32) -> Result<(), NetError> {
    if hidden == 0 || hidden > MAX_HIDDEN || !hidden.is_multiple_of(HIDDEN_MULTIPLE) {
        return Err(NetError::BadHiddenSize(hidden));
    }
    if eval_scale == 0 || eval_scale > MAX_EVAL_SCALE {
        return Err(NetError::BadEvalScale(eval_scale));
    }
    Ok(())
}

// ANCHOR: activation
/// A hidden value clipped to 0..1, at the scale QA: 0..255.
fn clipped(value: i16) -> i32 {
    i32::from(value).clamp(0, QA)
}

/// The output layer's sum with CReLU: each clipped hidden value times its weight.
fn crelu_dot(values: &[i16], weights: &[i16]) -> i64 {
    values
        .iter()
        .zip(weights)
        .map(|(&v, &w)| i64::from(clipped(v) * i32::from(w)))
        .sum()
}

/// The output layer's sum with SCReLU: each clipped hidden value squared, times its weight.
/// Computed as `(a × w) × a`: `a × w` is at most 255 × 127 = 32,385 and fits in an `i16`.
fn screlu_dot(values: &[i16], weights: &[i16]) -> i64 {
    values
        .iter()
        .zip(weights)
        .map(|(&v, &w)| {
            let a = clipped(v);
            i64::from(a * i32::from(w)) * i64::from(a)
        })
        .sum()
}
// ANCHOR_END: activation

/// Adds a row of weights to the hidden values. Wrapping, so an impossible position can't panic.
fn add(values: &mut [i16], row: &[i16]) {
    for (value, &weight) in values.iter_mut().zip(row) {
        *value = value.wrapping_add(weight);
    }
}

/// Subtracts a row of weights from the hidden values.
fn subtract(values: &mut [i16], row: &[i16]) {
    for (value, &weight) in values.iter_mut().zip(row) {
        *value = value.wrapping_sub(weight);
    }
}

/// FNV-1a, 64-bit: a simple checksum (Fowler, Noll and Vo). Good at catching damaged files.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use cb2026_chess::{Move, STARTING_FEN};

    use super::*;
    use crate::float::WEIGHT_LIMIT;
    use crate::test_support::{
        mirror, random_float_network, random_game_positions, special_move_positions,
    };

    fn random_network(hidden: usize, activation: Activation, seed: u64) -> Network {
        random_float_network(hidden, activation, seed)
            .quantize()
            .unwrap()
    }

    #[test]
    fn the_limits_are_the_weight_limit_at_each_scale() {
        assert_eq!(MAX_INPUT_WEIGHT, (WEIGHT_LIMIT * QA as f32).round() as i16);
        assert_eq!(MAX_OUTPUT_WEIGHT, (WEIGHT_LIMIT * QB as f32).round() as i16);
        // The bounds in the module docs.
        assert!(33 * i32::from(MAX_INPUT_WEIGHT) <= i32::from(i16::MAX));
        assert!(QA * i32::from(MAX_OUTPUT_WEIGHT) <= i32::from(i16::MAX));
    }

    #[test]
    fn a_network_survives_a_round_trip_through_its_file() {
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            let net = random_network(DEFAULT_HIDDEN, activation, 7);
            let bytes = net.to_bytes();
            assert_eq!(bytes.len(), 48 + 2 * (768 * 256 + 256 + 512) + 4);
            assert_eq!(Network::from_bytes(&bytes), Ok(net.clone()));
            assert_eq!(
                u64::from_le_bytes(bytes[40..48].try_into().unwrap()),
                net.checksum()
            );
        }
    }

    /// Sets header field `n` (0 = version) and recomputes the checksum.
    fn with_field(bytes: &[u8], n: usize, value: u32) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        bytes[8 + 4 * n..12 + 4 * n].copy_from_slice(&value.to_le_bytes());
        bytes
    }

    fn reseal(mut bytes: Vec<u8>) -> Vec<u8> {
        let checksum = fnv1a(&bytes[48..]);
        bytes[40..48].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    #[test]
    fn damaged_or_foreign_files_are_refused() {
        let good = random_network(DEFAULT_HIDDEN, Activation::SquaredClippedRelu, 8).to_bytes();
        let read = Network::from_bytes;
        assert!(matches!(
            read(&good[..40]),
            Err(NetError::WrongLength { .. })
        ));
        assert!(matches!(
            read(&good[..good.len() - 1]),
            Err(NetError::WrongLength { .. })
        ));
        let mut foreign = good.clone();
        foreign[0] = b'X';
        assert_eq!(read(&foreign), Err(NetError::BadMagic));
        assert_eq!(
            read(&with_field(&good, 0, 2)),
            Err(NetError::UnsupportedVersion(2))
        );
        assert_eq!(
            read(&with_field(&good, 1, 1)),
            Err(NetError::UnsupportedInputs(1))
        );
        assert_eq!(
            read(&with_field(&good, 2, 250)),
            Err(NetError::BadHiddenSize(250))
        );
        assert_eq!(
            read(&with_field(&good, 3, 9)),
            Err(NetError::UnknownActivation(9))
        );
        assert_eq!(
            read(&with_field(&good, 4, 8)),
            Err(NetError::UnsupportedOutputBuckets(8))
        );
        assert_eq!(
            read(&with_field(&good, 5, 127)),
            Err(NetError::WrongScales { qa: 127, qb: 64 })
        );
        assert_eq!(
            read(&with_field(&good, 7, 0)),
            Err(NetError::BadEvalScale(0))
        );
        let mut damaged = good.clone();
        damaged[1000] ^= 1;
        assert_eq!(read(&damaged), Err(NetError::BadChecksum));
        // A first-layer weight of 600 with a correct checksum: refused for its size.
        let mut too_big = good.clone();
        too_big[48..50].copy_from_slice(&600i16.to_le_bytes());
        assert_eq!(read(&reseal(too_big)), Err(NetError::WeightOutOfRange));
    }

    #[test]
    fn hidden_sizes_must_be_a_multiple_of_32_up_to_4096() {
        for (hidden, ok) in [
            (0, false),
            (16, false),
            (32, true),
            (250, false),
            (4096, true),
            (4128, false),
        ] {
            assert_eq!(check_shape(hidden, 146).is_ok(), ok, "{hidden}");
        }
    }

    #[test]
    fn updating_move_by_move_matches_computing_from_scratch() {
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            // Exactness doesn't depend on the layer's size; a small one keeps the test quick.
            let net = random_network(64, activation, 9);
            let positions = special_move_positions()
                .into_iter()
                .chain(random_game_positions(11, 20, 160));
            for pos in positions {
                let acc = net.accumulator(&pos);
                for &mv in pos.legal_moves().iter() {
                    let mut child = pos.clone();
                    child.make_move(mv);
                    let mut updated = acc.clone();
                    net.update(&mut updated, &Changes::of(&pos, mv));
                    assert_eq!(
                        updated,
                        net.accumulator(&child),
                        "{mv:?} in {}",
                        pos.to_fen()
                    );
                }
            }
        }
    }

    #[test]
    fn a_game_played_out_keeps_its_accumulator_exact() {
        // A long line of updates, not just one: errors would pile up.
        let net = random_network(64, Activation::SquaredClippedRelu, 10);
        let mut pos = Position::from_fen(STARTING_FEN).unwrap();
        let mut acc = net.accumulator(&pos);
        let mut rng = crate::test_support::Rng::new(12);
        for _ in 0..200 {
            let moves = pos.legal_moves();
            if moves.is_empty() {
                break;
            }
            let mv: Move = moves[rng.below(moves.len())];
            net.update(&mut acc, &Changes::of(&pos, mv));
            pos.make_move(mv);
            assert_eq!(acc, net.accumulator(&pos));
        }
    }

    #[test]
    fn mirrored_positions_score_exactly_the_same() {
        for activation in [Activation::ClippedRelu, Activation::SquaredClippedRelu] {
            let net = random_network(DEFAULT_HIDDEN, activation, 13);
            for pos in random_game_positions(14, 20, 120) {
                assert_eq!(
                    net.evaluate_position(&pos),
                    net.evaluate_position(&mirror(&pos)),
                    "{}",
                    pos.to_fen()
                );
            }
        }
    }

    #[test]
    fn the_largest_weights_do_not_overflow() {
        // Every weight and bias at the limit: the start position's 32 pieces and the bias give
        // 33 × 505 in every hidden value, the most a legal position can reach.
        for sign in [1.0, -1.0] {
            let mut float = FloatNetwork::zeros(
                DEFAULT_HIDDEN,
                Activation::SquaredClippedRelu,
                MAX_EVAL_SCALE,
            );
            float.input_weights.fill(sign * WEIGHT_LIMIT);
            float.input_biases.fill(sign * WEIGHT_LIMIT);
            float.output_weights.fill(WEIGHT_LIMIT);
            let net = float.quantize().unwrap();
            let pos = Position::from_fen(STARTING_FEN).unwrap();
            let acc = net.accumulator(&pos);
            let peak = sign as i16 * 33 * MAX_INPUT_WEIGHT;
            assert!(acc.view(Color::White).iter().all(|&v| v == peak));
            // Positive: every activation is 255, so the output is 2 × 256 × 255² × 127 / 255.
            let expected_sum = if sign > 0.0 { 2 * 256 * 255 * 127 } else { 0 };
            let expected = expected_sum * i64::from(MAX_EVAL_SCALE) / i64::from(QA * QB);
            assert_eq!(i64::from(net.evaluate(&acc, Color::White)), expected);
        }
    }

    #[test]
    fn the_start_position_scores_the_same_for_either_side_to_move() {
        // The start position is symmetric, so both views are equal and so is the score.
        let net = random_network(DEFAULT_HIDDEN, Activation::ClippedRelu, 15);
        let pos = Position::from_fen(STARTING_FEN).unwrap();
        let acc = net.accumulator(&pos);
        assert_eq!(acc.view(Color::White), acc.view(Color::Black));
        assert_eq!(
            net.evaluate(&acc, Color::White),
            net.evaluate(&acc, Color::Black)
        );
    }
}
