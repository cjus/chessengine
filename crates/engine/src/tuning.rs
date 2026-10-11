//! Support for tuning the evaluation: the evaluation written as a dot product.
//!
//! Every term of the evaluation is "parameter *p*, *n* times, for one side" (see
//! [`crate::eval::collect`]). So for a given position, the evaluation is
//!
//! ```text
//! mg = Σ_p coefficient_p × value_p.mg        eg = Σ_p coefficient_p × value_p.eg
//! score = (mg × phase + eg × (24 − phase)) / 24
//! ```
//!
//! where each **coefficient** is how many times White gets parameter *p* minus how many times
//! Black does. The one exception is king danger, which grows with the *square* of the attacks;
//! it is kept as a constant computed with the current values.
//!
//! [`features`] records the coefficients once per position. After that, evaluating with any
//! parameter values is a short sum, and so is the gradient. That is what makes tuning hundreds
//! of parameters on hundreds of thousands of positions practical (stage 16, `cb2026-tools`).

use cb2026_chess::{Color, Position};

use crate::eval::{self, EvalParams, MAX_PHASE, Param, TermSink};

/// A position's evaluation features: which parameters apply, how often, from White's side.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Features {
    /// `(parameter index, coefficient)` pairs, sorted by index, without zeros.
    pub coefficients: Vec<(u16, i16)>,
    /// The king danger penalty, White's minus Black's (a middlegame constant, see the module
    /// docs).
    pub constant_mg: i32,
    /// The game phase.
    pub phase: i32,
}

/// Records coefficients as the evaluation reports its terms.
#[derive(Default)]
struct FeatureSink {
    dense: Vec<i32>,
    constant_mg: i32,
}

impl TermSink for FeatureSink {
    fn term(&mut self, side: Color, param: Param, count: i32) {
        let sign = if side == Color::White { 1 } else { -1 };
        self.dense[param.index()] += sign * count;
    }

    fn king_danger(&mut self, side: Color, penalty: i32) {
        let sign = if side == Color::White { 1 } else { -1 };
        self.constant_mg -= sign * penalty;
    }
}

/// The features of `pos` under `params` (only the switched-on terms appear).
pub fn features(pos: &Position, params: &EvalParams) -> Features {
    let mut sink = FeatureSink {
        dense: vec![0; Param::COUNT],
        constant_mg: 0,
    };
    eval::collect(pos, params, &mut sink);
    let coefficients = sink
        .dense
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c != 0)
        .map(|(i, &c)| (i as u16, c as i16))
        .collect();
    Features {
        coefficients,
        constant_mg: sink.constant_mg,
        phase: eval::phase(pos, params),
    }
}

/// The middlegame and endgame sums for `values` (one `(mg, eg)` per [`Param::index`]).
pub fn mg_eg(features: &Features, values: &[(f64, f64)]) -> (f64, f64) {
    let mut mg = f64::from(features.constant_mg);
    let mut eg = 0.0;
    for &(index, coefficient) in &features.coefficients {
        let (v_mg, v_eg) = values[index as usize];
        mg += f64::from(coefficient) * v_mg;
        eg += f64::from(coefficient) * v_eg;
    }
    (mg, eg)
}

/// The tapered evaluation from White's side for `values`, without rounding.
pub fn evaluate(features: &Features, values: &[(f64, f64)]) -> f64 {
    let (mg, eg) = mg_eg(features, values);
    let phase = f64::from(features.phase);
    let max = f64::from(MAX_PHASE);
    (mg * phase + eg * (max - phase)) / max
}

/// Every parameter's value in `params`, by [`Param::index`], as floating point.
pub fn values(params: &EvalParams) -> Vec<(f64, f64)> {
    Param::all()
        .into_iter()
        .map(|p| {
            let (mg, eg) = params.value(p);
            (f64::from(mg), f64::from(eg))
        })
        .collect()
}

/// `params` with every parameter replaced by `values`, rounded to whole centipawns.
pub fn with_values(params: &EvalParams, values: &[(f64, f64)]) -> EvalParams {
    let mut out = params.clone();
    for (param, &(mg, eg)) in Param::all().into_iter().zip(values) {
        out.set_value(param, (mg.round() as i32, eg.round() as i32));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_indices_are_a_numbering() {
        let all = Param::all();
        assert_eq!(all.len(), Param::COUNT);
        for (i, p) in all.iter().enumerate() {
            assert_eq!(p.index(), i, "{p:?}");
        }
    }

    #[test]
    fn features_reproduce_the_evaluation() {
        let params = EvalParams::default();
        let values = values(&params);
        let mut checked = 0;
        for fen in crate::bench::BENCH_POSITIONS {
            // The position and every position one move later.
            let pos = Position::from_fen(fen).unwrap();
            let mut positions = vec![pos.clone()];
            for &mv in pos.legal_moves().iter() {
                let mut child = pos.clone();
                child.make_move(mv);
                positions.push(child);
            }
            for p in positions {
                let exact = eval::trace(&p, &params).total;
                let from_features = evaluate(&features(&p, &params), &values);
                // The evaluation rounds toward zero; the features don't round.
                assert!(
                    (f64::from(exact) - from_features).abs() < 1.0,
                    "{}: {exact} vs {from_features}",
                    p.to_fen()
                );
                checked += 1;
            }
        }
        assert!(checked > 300);
    }

    #[test]
    fn values_round_trip() {
        let params = EvalParams::default();
        assert_eq!(with_values(&params, &values(&params)), params);
    }
}
