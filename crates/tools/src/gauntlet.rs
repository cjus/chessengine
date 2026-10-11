//! Rating gauntlets: where does the engine stand on a rating list?
//!
//! An SPRT compares two versions of cb2026 with each other. To place cb2026 on an outside
//! scale, like the CCRL Blitz list, it plays a **gauntlet**: a match against each of several
//! reference engines whose ratings on that list are known, chosen so their ratings bracket
//! ours. Each match says "cb2026 scored S against an engine rated r".
//!
//! [`fit_rating`] turns those results into one rating: the **maximum likelihood** estimate,
//! the rating `R` that makes the results most probable under the logistic Elo model. That's
//! the `R` where the total expected score equals the total actual score,
//!
//! ```text
//! Σ games_i × E(R − r_i) = Σ score_i,     E(d) = 1 / (1 + 10^(−d/400))
//! ```
//!
//! found by bisection (the left side grows with `R`). The error margin comes from the
//! curvature of the likelihood, using each match's observed per-game variance, so draws (which
//! carry less information than wins and losses) are accounted for. This is what rating tools
//! like Ordo and BayesElo do, simplified to one unknown rating: the reference ratings are held
//! fixed.
//!
//! The result is on the reference list's scale only as far as the time control and hardware
//! match the list's; a gauntlet at 8+0.08 on the test Mac is an approximation of a CCRL Blitz
//! rating, not a CCRL rating.
//!
//! See the Chess Programming Wiki, [Match Statistics](https://www.chessprogramming.org/Match_Statistics)
//! and [Elo Rating System](https://www.chessprogramming.org/Elo_Rating_System).

use crate::sprt::{Score, expected_score};

/// One match of a gauntlet: the opponent's rating and our score against it.
#[derive(Clone, Debug, PartialEq)]
pub struct GauntletResult {
    /// The opponent's name.
    pub name: String,
    /// The opponent's rating on the reference list.
    pub rating: f64,
    /// Our wins, draws and losses against it.
    pub score: Score,
}

/// Our rating and its 95% error margin, from a gauntlet's results.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RatingFit {
    /// A rating with its 95% margin.
    Rating(f64, f64),
    /// We scored 100%: the rating is above this (the strongest opponent's) and can't be
    /// pinned down.
    Above(f64),
    /// We scored 0%: the rating is below the weakest opponent's.
    Below(f64),
}

/// The maximum likelihood rating for `results` (see the module documentation), or `None`
/// with no games.
pub fn fit_rating(results: &[GauntletResult]) -> Option<RatingFit> {
    let played: Vec<&GauntletResult> = results.iter().filter(|r| r.score.games() > 0).collect();
    if played.is_empty() {
        return None;
    }
    let total_games: f64 = played.iter().map(|r| f64::from(r.score.games())).sum();
    let total_score: f64 = played.iter().map(|r| points(&r.score)).sum();
    let ratings = played.iter().map(|r| r.rating);
    if total_score <= 0.0 {
        return Some(RatingFit::Below(ratings.fold(f64::INFINITY, f64::min)));
    }
    if total_score >= total_games {
        return Some(RatingFit::Above(ratings.fold(f64::NEG_INFINITY, f64::max)));
    }
    let expected = |rating: f64| -> f64 {
        played
            .iter()
            .map(|r| f64::from(r.score.games()) * expected_score(rating - r.rating))
            .sum()
    };
    // Bisection: the expected total grows with the rating.
    let (mut low, mut high) = (-2000.0, 6000.0);
    for _ in 0..100 {
        let mid = (low + high) / 2.0;
        if expected(mid) < total_score {
            low = mid;
        } else {
            high = mid;
        }
    }
    let rating = (low + high) / 2.0;

    // The standard error: sqrt(Σ n_i v_i) / Σ n_i E_i (1 − E_i) × 400 / ln 10, where v_i is
    // the per-game variance actually observed against opponent i.
    let slope = std::f64::consts::LN_10 / 400.0;
    let mut information = 0.0;
    let mut variance = 0.0;
    for r in &played {
        let n = f64::from(r.score.games());
        let e = expected_score(rating - r.rating);
        information += n * e * (1.0 - e) * slope;
        variance += n * observed_variance(&r.score).max(1e-9);
    }
    let error = variance.sqrt() / information;
    Some(RatingFit::Rating(rating, 1.96 * error))
}

fn points(score: &Score) -> f64 {
    f64::from(score.wins) + f64::from(score.draws) / 2.0
}

/// The per-game variance of the results in `score`.
fn observed_variance(score: &Score) -> f64 {
    let n = f64::from(score.games());
    let mean = points(score) / n;
    (f64::from(score.wins) * (1.0 - mean).powi(2)
        + f64::from(score.draws) * (0.5 - mean).powi(2)
        + f64::from(score.losses) * mean.powi(2))
        / n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(rating: f64, wins: u32, draws: u32, losses: u32) -> GauntletResult {
        GauntletResult {
            name: format!("engine {rating}"),
            rating,
            score: Score {
                wins,
                draws,
                losses,
            },
        }
    }

    #[test]
    fn an_even_score_means_the_same_rating() {
        let fit = fit_rating(&[result(2000.0, 40, 20, 40)]).unwrap();
        let RatingFit::Rating(rating, margin) = fit else {
            panic!("{fit:?}");
        };
        assert!((rating - 2000.0).abs() < 0.01, "{rating}");
        // About ±60 for 100 games with 20% draws.
        assert!(margin > 40.0 && margin < 80.0, "{margin}");
    }

    #[test]
    fn matches_the_one_opponent_formula() {
        // 75% against a 2000 engine is +191 Elo.
        let RatingFit::Rating(rating, _) = fit_rating(&[result(2000.0, 70, 10, 20)]).unwrap()
        else {
            panic!();
        };
        assert!((rating - 2190.8).abs() < 0.5, "{rating}");
    }

    #[test]
    fn combines_opponents_and_narrows_with_games() {
        // 50% against both a 1900 and a 2100 engine: about 2000.
        let results = [result(1900.0, 30, 40, 30), result(2100.0, 30, 40, 30)];
        let Some(RatingFit::Rating(rating, margin)) = fit_rating(&results) else {
            panic!();
        };
        assert!((rating - 2000.0).abs() < 1.0, "{rating}");
        let more = [result(1900.0, 300, 400, 300), result(2100.0, 300, 400, 300)];
        let Some(RatingFit::Rating(_, narrower)) = fit_rating(&more) else {
            panic!();
        };
        assert!(
            narrower < margin / 3.0 && narrower > margin / 3.3,
            "{narrower} vs {margin}"
        );
    }

    #[test]
    fn all_wins_or_losses_give_a_bound() {
        assert_eq!(
            fit_rating(&[result(1800.0, 10, 0, 0), result(2000.0, 5, 0, 0)]),
            Some(RatingFit::Above(2000.0))
        );
        assert_eq!(
            fit_rating(&[result(1800.0, 0, 0, 10), result(2000.0, 0, 0, 3)]),
            Some(RatingFit::Below(1800.0))
        );
        assert_eq!(fit_rating(&[result(1800.0, 0, 0, 0)]), None);
    }
}
