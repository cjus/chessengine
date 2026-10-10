//! Match statistics: Elo estimates and the sequential probability ratio test (SPRT).
//!
//! ## Why a *sequential* test
//!
//! "Is version B stronger than version A?" needs many games: chess results are noisy, and
//! the changes engines test are usually worth a few Elo. Playing a fixed number of games
//! wastes time on clear results and stops too early on close ones. An SPRT instead checks
//! after every game whether the evidence is already strong enough to decide between two
//! hypotheses:
//!
//! - **H0:** the Elo difference is `elo0` (often 0: "no better");
//! - **H1:** the Elo difference is `elo1` (for example 5: "at least this much better").
//!
//! It tracks the *log-likelihood ratio* (LLR) of H1 against H0. When the LLR rises above the
//! upper bound, H1 is accepted (the change passes); below the lower bound, H0 is accepted (it
//! fails). With error rates α = β = 0.05, the bounds are about ±2.94. Clear results end in a
//! few hundred games, close ones take longer, and the error rates stay as chosen.
//!
//! ## The formula used
//!
//! With W wins, D draws and L losses in N games, the score is `s = (W + D/2) / N` and its
//! per-game variance is `var = (W(1−s)² + D(½−s)² + L(0−s)²) / N`. With `p(e)` the expected
//! score of an Elo advantage `e` (the logistic curve `1 / (1 + 10^(−e/400))`), the
//! log-likelihood ratio is approximately
//!
//! ```text
//! LLR ≈ N · (p1 − p0) · (2s − p0 − p1) / (2 · var)
//! ```
//!
//! This is the "trinomial GSPRT" approximation used by Fishtest and cutechess-style tools.
//! (Fishtest and fastchess now use game *pairs*, the pentanomial model, which is a little more
//! efficient; that's a possible improvement here.)
//!
//! See the Chess Programming Wiki, [Sequential Probability Ratio Test](https://www.chessprogramming.org/Sequential_Probability_Ratio_Test)
//! and [Match Statistics](https://www.chessprogramming.org/Match_Statistics).

use std::fmt;

/// Wins, draws and losses from the first engine's point of view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    /// Games the first engine won.
    pub wins: u32,
    /// Drawn games.
    pub draws: u32,
    /// Games the first engine lost.
    pub losses: u32,
}

/// The expected score of an Elo advantage `elo` (the logistic model).
pub fn expected_score(elo: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf(-elo / 400.0))
}

/// The Elo advantage that gives expected score `score` (the inverse of [`expected_score`]).
pub fn elo_from_score(score: f64) -> f64 {
    -400.0 * (1.0 / score - 1.0).log10()
}

impl Score {
    /// Games played.
    pub fn games(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    /// The first engine's score, 0 to 1 (draws count half).
    pub fn ratio(&self) -> f64 {
        (self.wins as f64 + self.draws as f64 / 2.0) / self.games() as f64
    }

    /// The per-game variance of the score.
    fn variance(&self) -> f64 {
        let n = self.games() as f64;
        let s = self.ratio();
        (self.wins as f64 * (1.0 - s).powi(2)
            + self.draws as f64 * (0.5 - s).powi(2)
            + self.losses as f64 * s.powi(2))
            / n
    }

    /// The Elo difference and its 95% error margin, or `None` until both sides have scored
    /// (an all-wins or all-losses score has no finite Elo).
    pub fn elo(&self) -> Option<(f64, f64)> {
        let n = self.games() as f64;
        let s = self.ratio();
        if self.games() == 0 || s <= 0.0 || s >= 1.0 {
            return None;
        }
        let margin = 1.96 * (self.variance() / n).sqrt();
        let low = elo_from_score((s - margin).max(1e-6));
        let high = elo_from_score((s + margin).min(1.0 - 1e-6));
        Some((elo_from_score(s), (high - low) / 2.0))
    }

    // ANCHOR: llr
    /// The log-likelihood ratio of "the Elo difference is `elo1`" against "it is `elo0`".
    ///
    /// If every game so far had the same result, the variance is zero and the formula breaks
    /// down; one imaginary draw is added in that case, so a long run of wins (or losses) still
    /// ends the test after a reasonable number of games.
    pub fn llr(&self, elo0: f64, elo1: f64) -> f64 {
        if self.games() == 0 {
            return 0.0;
        }
        let one_sided = self.wins == self.games() || self.losses == self.games();
        let score = if one_sided {
            Score {
                draws: self.draws + 1,
                ..*self
            }
        } else {
            *self
        };
        let n = score.games() as f64;
        let variance = score.variance();
        if variance == 0.0 {
            // Only draws: no evidence either way.
            return 0.0;
        }
        let (p0, p1) = (expected_score(elo0), expected_score(elo1));
        n * (p1 - p0) * (2.0 * score.ratio() - p0 - p1) / (2.0 * variance)
    }
    // ANCHOR_END: llr
}

impl fmt::Display for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} games: +{} -{} ={}",
            self.games(),
            self.wins,
            self.losses,
            self.draws
        )?;
        match self.elo() {
            Some((elo, margin)) => write!(f, "  Elo {elo:+.1} ± {margin:.1}"),
            None => write!(f, "  Elo n/a"),
        }
    }
}

/// The settings of a sequential probability ratio test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprt {
    /// The Elo difference under the null hypothesis.
    pub elo0: f64,
    /// The Elo difference under the alternative hypothesis.
    pub elo1: f64,
    /// The chance of accepting H1 when H0 is true.
    pub alpha: f64,
    /// The chance of accepting H0 when H1 is true.
    pub beta: f64,
}

/// What an SPRT has concluded so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not enough evidence yet: keep playing.
    Continue,
    /// H1 accepted: the change passes.
    Pass,
    /// H0 accepted: the change fails.
    Fail,
}

impl Sprt {
    /// A test of `[elo0, elo1]` with the usual error rates α = β = 0.05.
    pub fn new(elo0: f64, elo1: f64) -> Sprt {
        Sprt {
            elo0,
            elo1,
            alpha: 0.05,
            beta: 0.05,
        }
    }

    /// The LLR bounds `(lower, upper)`.
    pub fn bounds(&self) -> (f64, f64) {
        (
            (self.beta / (1.0 - self.alpha)).ln(),
            ((1.0 - self.beta) / self.alpha).ln(),
        )
    }

    /// The verdict for `score` so far.
    pub fn verdict(&self, score: &Score) -> Verdict {
        let llr = score.llr(self.elo0, self.elo1);
        let (lower, upper) = self.bounds();
        if llr >= upper {
            Verdict::Pass
        } else if llr <= lower {
            Verdict::Fail
        } else {
            Verdict::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_score_and_back() {
        assert!((expected_score(0.0) - 0.5).abs() < 1e-12);
        assert!((expected_score(400.0) - 10.0 / 11.0).abs() < 1e-12);
        for elo in [-300.0, -50.0, 0.0, 25.0, 200.0] {
            assert!((elo_from_score(expected_score(elo)) - elo).abs() < 1e-9);
        }
    }

    #[test]
    fn elo_estimate() {
        let even = Score {
            wins: 30,
            draws: 40,
            losses: 30,
        };
        let (elo, margin) = even.elo().unwrap();
        assert!(elo.abs() < 1e-9);
        assert!(margin > 30.0 && margin < 60.0, "{margin}");
        // 75% score: about +191 Elo.
        let strong = Score {
            wins: 60,
            draws: 30,
            losses: 10,
        };
        assert!((strong.elo().unwrap().0 - 190.8).abs() < 0.1);
        assert_eq!(
            Score {
                wins: 5,
                ..Score::default()
            }
            .elo(),
            None
        );
    }

    #[test]
    fn sprt_bounds_and_verdicts() {
        let sprt = Sprt::new(0.0, 5.0);
        let (lower, upper) = sprt.bounds();
        assert!((lower + 2.944).abs() < 0.001 && (upper - 2.944).abs() < 0.001);
        // A 55% score (about +35 Elo) over 4,000 games: clearly better than 0.
        let clearly_better = Score {
            wins: 1600,
            draws: 1200,
            losses: 1200,
        };
        assert_eq!(sprt.verdict(&clearly_better), Verdict::Pass);
        let clearly_worse = Score {
            wins: 1200,
            draws: 1200,
            losses: 1600,
        };
        assert_eq!(sprt.verdict(&clearly_worse), Verdict::Fail);
        let too_early = Score {
            wins: 5,
            draws: 5,
            losses: 4,
        };
        assert_eq!(sprt.verdict(&too_early), Verdict::Continue);
    }

    #[test]
    fn a_run_of_wins_or_losses_ends_the_test() {
        let sprt = Sprt::new(0.0, 5.0);
        let wins = |n| Score {
            wins: n,
            ..Score::default()
        };
        assert_eq!(sprt.verdict(&wins(5)), Verdict::Continue);
        assert_eq!(sprt.verdict(&wins(30)), Verdict::Pass);
        let losses = Score {
            losses: 30,
            ..Score::default()
        };
        assert_eq!(sprt.verdict(&losses), Verdict::Fail);
        let draws = Score {
            draws: 100,
            ..Score::default()
        };
        assert_eq!(sprt.verdict(&draws), Verdict::Continue);
    }

    #[test]
    fn displays_a_summary() {
        let score = Score {
            wins: 6,
            draws: 2,
            losses: 2,
        };
        assert!(
            score
                .to_string()
                .starts_with("10 games: +6 -2 =2  Elo +147")
        );
    }
}
