//! Texel tuning: fitting the evaluation's parameters to game results.
//!
//! An evaluation score says how good a position is; a game result says how it actually ended.
//! Over many positions the two should agree: positions scored +2 should mostly have been won.
//! Texel tuning (named after Peter Österlund's engine Texel, where it was popularised) turns
//! that into an optimisation problem:
//!
//! 1. Map a score to an expected result with a **sigmoid**: `σ(s) = 1 / (1 + 10^(−K·s/400))`.
//!    A score of 0 expects ½, large positive scores expect a win.
//! 2. The **error** is the mean squared difference between the expected and the actual
//!    results, over all positions.
//! 3. First fit `K` with the current parameters (it only fixes the scale between centipawns
//!    and results), then adjust the parameters to minimise the error.
//!
//! Because the evaluation is a dot product of features and parameters (see
//! `cb2026_engine::tuning`), the error's gradient with respect to every parameter is cheap to
//! compute, and the parameters can be fitted by gradient descent. cb2026 uses **Adam**, which
//! adapts the step size per parameter: rare parameters (a knight on a8) and common ones
//! (material) both converge.
//!
//! See the Chess Programming Wiki, [Texel's Tuning Method](https://www.chessprogramming.org/Texel%27s_Tuning_Method),
//! and Diederik Kingma and Jimmy Ba, "Adam: A Method for Stochastic Optimization" (2015).

use cb2026_engine::eval::MAX_PHASE;
use cb2026_engine::tuning::{self, Features};

/// A position's features and its game's result (White's side: 1, ½ or 0).
#[derive(Clone, Debug)]
pub struct TrainingPosition {
    /// The evaluation features.
    pub features: Features,
    /// The game result.
    pub result: f64,
}

/// The expected result for a score of `score` centipawns (White's side).
pub fn sigmoid(score: f64, k: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf(-k * score / 400.0))
}

// ANCHOR: error
/// The mean squared error between expected and actual results.
pub fn error(positions: &[TrainingPosition], values: &[(f64, f64)], k: f64) -> f64 {
    let total: f64 = positions
        .iter()
        .map(|p| {
            let expected = sigmoid(tuning::evaluate(&p.features, values), k);
            (p.result - expected).powi(2)
        })
        .sum();
    total / positions.len().max(1) as f64
}
// ANCHOR_END: error

/// The `K` that minimises the error for the current `values`, by golden-section search.
pub fn find_k(positions: &[TrainingPosition], values: &[(f64, f64)]) -> f64 {
    let (mut low, mut high) = (0.05, 5.0);
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..60 {
        let a = high - ratio * (high - low);
        let b = low + ratio * (high - low);
        if error(positions, values, a) < error(positions, values, b) {
            high = b;
        } else {
            low = a;
        }
    }
    (low + high) / 2.0
}

// ANCHOR: gradient
/// The error's gradient with respect to every parameter's middlegame and endgame value.
pub fn gradient(positions: &[TrainingPosition], values: &[(f64, f64)], k: f64) -> Vec<(f64, f64)> {
    let mut grad = vec![(0.0, 0.0); values.len()];
    let scale = k * std::f64::consts::LN_10 / 400.0;
    let max_phase = f64::from(MAX_PHASE);
    for p in positions {
        let s = sigmoid(tuning::evaluate(&p.features, values), k);
        // d(error)/d(score) for this position (the mean's 1/N is applied at the end).
        let d_score = -2.0 * (p.result - s) * s * (1.0 - s) * scale;
        let mg_share = f64::from(p.features.phase) / max_phase;
        for &(index, coefficient) in &p.features.coefficients {
            let c = f64::from(coefficient) * d_score;
            let g = &mut grad[index as usize];
            g.0 += c * mg_share;
            g.1 += c * (1.0 - mg_share);
        }
    }
    let n = positions.len().max(1) as f64;
    for g in &mut grad {
        g.0 /= n;
        g.1 /= n;
    }
    grad
}
// ANCHOR_END: gradient

/// How many positions each parameter appears in (with a non-zero coefficient).
pub fn occurrences(positions: &[TrainingPosition], count: usize) -> Vec<usize> {
    let mut seen = vec![0; count];
    for p in positions {
        for &(index, _) in &p.features.coefficients {
            seen[index as usize] += 1;
        }
    }
    seen
}

// ANCHOR: regularize
/// Adds L2 regularisation to `grad`: a pull of strength `lambda` back toward `start` for every
/// parameter, so a parameter only moves as far as the data can justify. Parameters seen in
/// fewer than `min_count` positions get no gradient at all and keep their starting values.
pub fn regularize(
    grad: &mut [(f64, f64)],
    values: &[(f64, f64)],
    start: &[(f64, f64)],
    seen: &[usize],
    lambda: f64,
    min_count: usize,
) {
    for i in 0..grad.len() {
        if seen[i] < min_count {
            grad[i] = (0.0, 0.0);
        } else {
            grad[i].0 += 2.0 * lambda * (values[i].0 - start[i].0);
            grad[i].1 += 2.0 * lambda * (values[i].1 - start[i].1);
        }
    }
}
// ANCHOR_END: regularize

/// The Adam optimiser's state.
#[derive(Clone, Debug)]
pub struct Adam {
    /// Step size, in centipawns.
    pub learning_rate: f64,
    first: Vec<(f64, f64)>,
    second: Vec<(f64, f64)>,
    steps: i32,
}

impl Adam {
    /// The moment decay rates and the small constant from the Adam paper.
    const BETA1: f64 = 0.9;
    const BETA2: f64 = 0.999;
    const EPSILON: f64 = 1e-8;

    /// A fresh optimiser for `count` parameters.
    pub fn new(count: usize, learning_rate: f64) -> Adam {
        Adam {
            learning_rate,
            first: vec![(0.0, 0.0); count],
            second: vec![(0.0, 0.0); count],
            steps: 0,
        }
    }

    // ANCHOR: adam
    /// Moves `values` one step against `grad`.
    pub fn step(&mut self, values: &mut [(f64, f64)], grad: &[(f64, f64)]) {
        self.steps += 1;
        let correction1 = 1.0 - Self::BETA1.powi(self.steps);
        let correction2 = 1.0 - Self::BETA2.powi(self.steps);
        let update = |value: &mut f64, g: f64, m: &mut f64, v: &mut f64| {
            *m = Self::BETA1 * *m + (1.0 - Self::BETA1) * g;
            *v = Self::BETA2 * *v + (1.0 - Self::BETA2) * g * g;
            let m_hat = *m / correction1;
            let v_hat = *v / correction2;
            *value -= self.learning_rate * m_hat / (v_hat.sqrt() + Self::EPSILON);
        };
        for i in 0..values.len() {
            let (m, v) = (&mut self.first[i], &mut self.second[i]);
            update(&mut values[i].0, grad[i].0, &mut m.0, &mut v.0);
            update(&mut values[i].1, grad[i].1, &mut m.1, &mut v.1);
        }
    }
    // ANCHOR_END: adam
}

/// `values` as the Rust source of `crates/engine/src/tuned.rs`, with `notes` (one line each)
/// in its documentation.
pub fn rust_source(values: &[(i32, i32)], notes: &[String]) -> String {
    use cb2026_engine::eval::Param;
    let mut out = String::new();
    out.push_str("//! The evaluation's tuned parameter values (stage 16).\n//!\n");
    out.push_str(
        "//! **Generated by the `tune` tool (`crates/tools/src/bin/tune.rs`): don't edit by\n",
    );
    out.push_str(
        "//! hand.** `EvalParams::tuned()` uses these values; `EvalParams::default()` keeps the\n",
    );
    out.push_str(
        "//! hand-written values from stages 9 and 15 until tuned values measure stronger.\n",
    );
    if !notes.is_empty() {
        out.push_str("//!\n");
        for note in notes {
            out.push_str(&format!("//! {note}\n"));
        }
    }
    out.push_str("\nuse crate::eval::{MgEg, Param};\n\n");
    out.push_str(
        "/// Every parameter's tuned `(middlegame, endgame)` value, in [`Param::index`] order.\n",
    );
    out.push_str("#[rustfmt::skip]\npub const TUNED: [MgEg; Param::COUNT] = [\n");
    let names = ["pawn", "knight", "bishop", "rook", "queen", "king"];
    let all = Param::all();
    let mut i = 0;
    while i < all.len() {
        let (heading, len) = match all[i] {
            Param::Material(_) => (
                "Material: pawn, knight, bishop, rook, queen, king".to_string(),
                6,
            ),
            Param::Pst(pt, _) => (
                format!(
                    "Piece-square table: {} (from White's side, rank 8 first)",
                    names[pt.index()]
                ),
                64,
            ),
            Param::Mobility(_) => (
                "Mobility per safe square: pawn, knight, bishop, rook, queen, king".to_string(),
                6,
            ),
            Param::Doubled => ("Doubled pawn, isolated pawn".to_string(), 2),
            Param::Passed(_) => ("Passed pawn by relative rank, 1 to 8".to_string(), 8),
            _ => (
                "Bishop pair, rook on open file, rook on semi-open file, pawn shield, tempo"
                    .to_string(),
                5,
            ),
        };
        out.push_str(&format!("    // {heading}\n"));
        for row in values[i..i + len].chunks(8) {
            let cells: Vec<String> = row
                .iter()
                .map(|(mg, eg)| format!("({mg:>4}, {eg:>4})"))
                .collect();
            out.push_str(&format!("    {},\n", cells.join(", ")));
        }
        i += len;
    }
    out.push_str("];\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb2026_chess::PieceType;
    use cb2026_chess::Position;
    use cb2026_engine::eval::{EvalParams, Param};

    fn training(fen: &str, result: f64, params: &EvalParams) -> TrainingPosition {
        TrainingPosition {
            features: tuning::features(&Position::from_fen(fen).unwrap(), params),
            result,
        }
    }

    #[test]
    fn sigmoid_shape() {
        assert!((sigmoid(0.0, 1.0) - 0.5).abs() < 1e-12);
        assert!(sigmoid(400.0, 1.0) > 0.9);
        assert!(sigmoid(-400.0, 1.0) < 0.1);
    }

    #[test]
    fn gradient_matches_finite_differences() {
        let params = EvalParams::default();
        let positions = vec![
            training("4k3/8/8/4N3/8/8/8/4K3 w - - 0 1", 1.0, &params),
            training("4k3/8/8/8/8/8/8/N3K3 w - - 0 1", 0.5, &params),
            training(cb2026_chess::STARTING_FEN, 0.5, &params),
        ];
        let values = tuning::values(&params);
        let grad = gradient(&positions, &values, 1.0);
        let h = 1e-3;
        for param in [Param::Material(PieceType::Knight), Param::Tempo] {
            let i = param.index();
            let mut up = values.clone();
            up[i].0 += h;
            let mut down = values.clone();
            down[i].0 -= h;
            let numeric = (error(&positions, &up, 1.0) - error(&positions, &down, 1.0)) / (2.0 * h);
            assert!(
                (numeric - grad[i].0).abs() < 1e-7,
                "{param:?}: {numeric} vs {}",
                grad[i].0
            );
        }
    }

    #[test]
    fn adam_reduces_the_error() {
        let params = EvalParams::default();
        // The knight is worth far less than we think: every knight-up position was drawn.
        let positions: Vec<_> = [
            "4k3/8/8/4N3/8/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/3N4/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/2N5/8/4K3 w - - 0 1",
        ]
        .iter()
        .map(|fen| training(fen, 0.5, &params))
        .collect();
        let mut values = tuning::values(&params);
        let before = error(&positions, &values, 1.0);
        let mut adam = Adam::new(values.len(), 5.0);
        for _ in 0..200 {
            let grad = gradient(&positions, &values, 1.0);
            adam.step(&mut values, &grad);
        }
        assert!(error(&positions, &values, 1.0) < before / 10.0);
    }

    #[test]
    fn k_fits_the_scale() {
        let params = EvalParams::default();
        // Positions a knight up that were won: the fitted K is positive and finite.
        let positions: Vec<_> = (0..4)
            .map(|_| training("4k3/8/8/4N3/8/8/8/4K3 w - - 0 1", 1.0, &params))
            .chain([training(cb2026_chess::STARTING_FEN, 0.5, &params)])
            .collect();
        let k = find_k(&positions, &tuning::values(&params));
        assert!(k > 0.05 && k <= 5.0, "{k}");
    }
}
