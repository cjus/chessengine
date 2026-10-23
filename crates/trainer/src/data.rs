//! The training records in memory.
//!
//! Records are kept as their raw 32 bytes, so 100 million take 3.2 GB of memory, and decoded
//! when a batch needs them. Every record is checked once, when the data is loaded, so a damaged
//! file is reported before hours of training rather than in the middle of them.
//!
//! The **validation set** is split off the end of the data. Files are written game by game, so
//! the end holds whole games the training never sees: positions from one game are too alike for
//! one in the training set and its neighbour in the validation set to be a fair test.

use cb2026_nnue::{RECORD_LEN, Record};

/// A set of training records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dataset {
    bytes: Vec<u8>,
}

impl Dataset {
    /// The records in `bytes` (one or more files' contents, joined), after checking each.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Dataset, String> {
        if !bytes.len().is_multiple_of(RECORD_LEN) {
            return Err(format!(
                "{} bytes is not a whole number of {RECORD_LEN}-byte records",
                bytes.len()
            ));
        }
        for (i, chunk) in bytes.chunks_exact(RECORD_LEN).enumerate() {
            let chunk: &[u8; RECORD_LEN] = chunk.try_into().expect("a whole record");
            Record::from_bytes(chunk).map_err(|e| format!("record {}: {e}", i + 1))?;
        }
        Ok(Dataset { bytes })
    }

    /// A dataset of these records.
    pub fn from_records(records: &[Record]) -> Dataset {
        Dataset {
            bytes: records.iter().flat_map(|r| r.to_bytes()).collect(),
        }
    }

    /// How many records there are.
    pub fn len(&self) -> usize {
        self.bytes.len() / RECORD_LEN
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Record `i`.
    pub fn record(&self, i: usize) -> Record {
        let bytes: &[u8; RECORD_LEN] = self.bytes[i * RECORD_LEN..(i + 1) * RECORD_LEN]
            .try_into()
            .expect("a whole record");
        Record::from_bytes(bytes).expect("checked when loaded")
    }

    /// Splits off the last `fraction` of the records as a validation set, keeping the rest.
    pub fn split_off_validation(&mut self, fraction: f64) -> Dataset {
        let keep = self.len() - (self.len() as f64 * fraction.clamp(0.0, 1.0)).round() as usize;
        Dataset {
            bytes: self.bytes.split_off(keep * RECORD_LEN),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::random_records;

    #[test]
    fn records_are_checked_and_split_from_the_end() {
        let records = random_records(5, 4);
        let mut data = Dataset::from_bytes(Dataset::from_records(&records).bytes).unwrap();
        assert_eq!(data.len(), records.len());
        assert_eq!(data.record(3), records[3]);
        let validation = data.split_off_validation(0.25);
        assert_eq!(data.len() + validation.len(), records.len());
        assert_eq!(
            validation.len(),
            (records.len() as f64 * 0.25).round() as usize
        );
        assert_eq!(validation.record(0), records[data.len()]);
    }

    #[test]
    fn damaged_data_is_refused() {
        let mut bytes = Dataset::from_records(&random_records(6, 1)).bytes;
        assert!(Dataset::from_bytes(bytes[..bytes.len() - 1].to_vec()).is_err());
        bytes[31] = 9; // the first record's version byte
        assert!(
            Dataset::from_bytes(bytes)
                .unwrap_err()
                .starts_with("record 1:")
        );
    }
}
