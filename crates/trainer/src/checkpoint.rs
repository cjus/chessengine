//! Checkpoints: training saved so that it can be resumed.
//!
//! Training on 100 million positions takes hours, so after every epoch the trainer saves the
//! floating-point network, AdamW's moment estimates and the number of epochs done. Resuming
//! from a checkpoint continues exactly where training left off: with the same data, settings
//! and number of threads it gives the same network as training straight through.
//!
//! The file is a 36-byte header (`CB2026TC`, the format version, the hidden layer's size, the
//! activation, the eval scale, the epochs done and AdamW's steps), then the network's weights,
//! the first moments and the second moments, each in the network's order, as little-endian
//! `f32`s. It isn't a network file: [`FloatNetwork::quantize`] makes one of those.

use cb2026_nnue::{Activation, FloatNetwork, INPUTS};

use crate::adam::AdamW;
use crate::backprop::Gradients;

const MAGIC: &[u8; 8] = b"CB2026TC";
const VERSION: u32 = 1;
/// The magic (8 bytes), five `u32` fields and AdamW's steps (`u64`).
const HEADER_LEN: usize = 36;

/// Training as it stands: the network, the optimiser and how far it has got.
#[derive(Clone, Debug, PartialEq)]
pub struct Checkpoint {
    /// The network.
    pub net: FloatNetwork,
    /// The optimiser and its moment estimates.
    pub adam: AdamW,
    /// Epochs completed.
    pub epochs_done: u32,
}

impl Checkpoint {
    /// The checkpoint as bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        let activation = match self.net.activation {
            Activation::ClippedRelu => 0u32,
            Activation::SquaredClippedRelu => 1,
        };
        for field in [
            VERSION,
            self.net.hidden as u32,
            activation,
            self.net.eval_scale,
            self.epochs_done,
        ] {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        bytes.extend_from_slice(&self.adam.steps.to_le_bytes());
        let mut floats = |values: &[f32]| {
            values
                .iter()
                .for_each(|v| bytes.extend_from_slice(&v.to_le_bytes()))
        };
        floats(&self.net.input_weights);
        floats(&self.net.input_biases);
        floats(&self.net.output_weights);
        floats(&[self.net.output_bias]);
        for moments in [&self.adam.first, &self.adam.second] {
            floats(&moments.input_weights);
            floats(&moments.input_biases);
            floats(&moments.output_weights);
            floats(&[moments.output_bias]);
        }
        bytes
    }

    /// Reads a checkpoint, with `learning_rate` and `weight_decay` for the optimiser from here on.
    pub fn from_bytes(
        bytes: &[u8],
        learning_rate: f32,
        weight_decay: f32,
    ) -> Result<Checkpoint, String> {
        if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
            return Err("not a cb2026 training checkpoint".to_string());
        }
        let field = |n: usize| {
            u32::from_le_bytes(bytes[8 + 4 * n..12 + 4 * n].try_into().expect("4 bytes"))
        };
        if field(0) != VERSION {
            return Err(format!(
                "checkpoint version {}; this trainer reads {VERSION}",
                field(0)
            ));
        }
        let hidden = field(1) as usize;
        let activation = match field(2) {
            0 => Activation::ClippedRelu,
            1 => Activation::SquaredClippedRelu,
            other => return Err(format!("unknown activation {other}")),
        };
        if hidden == 0 || hidden > cb2026_nnue::network::MAX_HIDDEN {
            return Err(format!("hidden layer size {hidden}"));
        }
        let per_set = INPUTS * hidden + hidden + 2 * hidden + 1;
        let expected = HEADER_LEN + 3 * 4 * per_set;
        if bytes.len() != expected {
            return Err(format!("expected {expected} bytes, found {}", bytes.len()));
        }
        let steps = u64::from_le_bytes(bytes[28..36].try_into().expect("8 bytes"));
        let mut floats = bytes[HEADER_LEN..]
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().expect("4 bytes")));
        let mut take = |n: usize| floats.by_ref().take(n).collect::<Vec<f32>>();
        let mut set = || Gradients {
            input_weights: take(INPUTS * hidden),
            input_biases: take(hidden),
            output_weights: take(2 * hidden),
            output_bias: take(1)[0],
        };
        let weights = set();
        let first = set();
        let second = set();
        let net = FloatNetwork {
            hidden,
            activation,
            eval_scale: field(3),
            input_weights: weights.input_weights,
            input_biases: weights.input_biases,
            output_weights: weights.output_weights,
            output_bias: weights.output_bias,
        };
        let adam = AdamW {
            learning_rate,
            weight_decay,
            steps,
            first,
            second,
        };
        Ok(Checkpoint {
            net,
            adam,
            epochs_done: field(4),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::random_network;

    #[test]
    fn a_checkpoint_survives_a_round_trip() {
        let mut adam = AdamW::new(32, 0.001, 0.01);
        adam.steps = 7;
        adam.first.input_weights[5] = 0.25;
        adam.second.output_bias = 0.5;
        let checkpoint = Checkpoint {
            net: random_network(32, Activation::ClippedRelu, 9),
            adam,
            epochs_done: 3,
        };
        let bytes = checkpoint.to_bytes();
        assert_eq!(Checkpoint::from_bytes(&bytes, 0.001, 0.01), Ok(checkpoint));
        assert!(Checkpoint::from_bytes(&bytes[..100], 0.001, 0.01).is_err());
        assert!(
            Checkpoint::from_bytes(b"CB2026NN and more bytes than a header", 0.001, 0.01).is_err()
        );
    }
}
