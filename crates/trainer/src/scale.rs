//! Fitting the eval scale: how centipawns turn into expected results.
//!
//! The network's output `y` is `y × eval scale` centipawns, and the trainer reads a score `cp`
//! as the expected result `σ(cp / eval scale)`. So the scale has to match how cb2026's
//! centipawns really relate to results. If it doesn't, the search's scores and the games'
//! results, the two halves of every target, disagree about what a centipawn is worth, and the
//! network's centipawns stretch away from the hand-crafted evaluation's, which the search's
//! pruning margins were tuned on.
//!
//! The fit is chapter 16's `K` in another form: the scale `s` that makes `σ(score / s)` predict
//! the records' results best, by least squares. The two are related by `s = 400 / (K ln 10)`;
//! on 2026-10-23 `K` was 1.19, a scale of about 146.
//!
//! The error is smooth with a single minimum in `s`, so a **golden-section search** finds it:
//! keep an interval that contains the minimum, and shrink it by the golden ratio each step by
//! comparing the error at two points inside it.

use crate::example::sigmoid;

/// The smallest eval scale the fit considers, in centipawns.
pub const MIN_EVAL_SCALE: f64 = 20.0;
/// The largest.
pub const MAX_EVAL_SCALE: f64 = 2000.0;
/// Golden-section steps: each shrinks the interval to 0.618 of its width, so 60 steps narrow
/// 20..2000 to well under a centipawn.
const STEPS: usize = 60;

// ANCHOR: fit
/// The eval scale that best predicts the results from the scores. `pairs` are (score in
/// centipawns, result as 0, ½ or 1), both from the same side's view.
pub fn fit_eval_scale(pairs: &[(f32, f32)]) -> f64 {
    let error = |scale: f64| -> f64 {
        pairs
            .iter()
            .map(|&(score, result)| {
                let expected = f64::from(sigmoid((f64::from(score) / scale) as f32));
                (expected - f64::from(result)).powi(2)
            })
            .sum()
    };
    let inverse_golden = (5f64.sqrt() - 1.0) / 2.0;
    let (mut low, mut high) = (MIN_EVAL_SCALE, MAX_EVAL_SCALE);
    let mut a = high - inverse_golden * (high - low);
    let mut b = low + inverse_golden * (high - low);
    let (mut error_a, mut error_b) = (error(a), error(b));
    for _ in 0..STEPS {
        // The minimum lies on the side of the smaller error.
        if error_a < error_b {
            high = b;
            (b, error_b) = (a, error_a);
            a = high - inverse_golden * (high - low);
            error_a = error(a);
        } else {
            low = a;
            (a, error_a) = (b, error_b);
            b = low + inverse_golden * (high - low);
            error_b = error(b);
        }
    }
    (low + high) / 2.0
}
// ANCHOR_END: fit

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fit_finds_the_scale_that_made_the_results() {
        // Results exactly as a scale of 146 predicts.
        let pairs: Vec<(f32, f32)> = (-80..=80)
            .map(|i| {
                let score = i as f32 * 10.0;
                (score, sigmoid(score / 146.0))
            })
            .collect();
        assert!((fit_eval_scale(&pairs) - 146.0).abs() < 0.5);
    }

    #[test]
    fn the_fit_works_with_won_drawn_and_lost_games() {
        // Results of 0, ½ or 1, drawn at random so that their average matches a scale of 300.
        let mut pairs = Vec::new();
        let mut seed = 12345u64;
        for i in 0..20_000 {
            let score = ((i % 161) as f32 - 80.0) * 10.0;
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let roll = (seed >> 40) as f32 / (1u64 << 24) as f32;
            let p = sigmoid(score / 300.0);
            // Draws up to a fifth of the time; wins at p − draws/2, so the expected result is p.
            let draw = 0.2f32.min(2.0 * p.min(1.0 - p));
            let result = if roll < draw {
                0.5
            } else if roll < draw + (p - draw / 2.0) {
                1.0
            } else {
                0.0
            };
            pairs.push((score, result));
        }
        let fitted = fit_eval_scale(&pairs);
        assert!((fitted - 300.0).abs() < 30.0, "fitted {fitted}");
    }
}
