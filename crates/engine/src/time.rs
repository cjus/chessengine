//! Time management: how long to think about a move.
//!
//! With a clock, the engine has to share its time across all the moves still to come. Think
//! too little and it plays weak moves; too long and it runs short later (or loses on time).
//!
//! cb2026 sets two limits for each move:
//!
//! - **Soft limit:** after finishing a depth, don't start the next one if this much time has
//!   passed. The next depth usually takes several times longer than the last, so starting it
//!   late mostly wastes the time.
//! - **Hard limit:** stop the search immediately, mid-depth, at this point. It is a safety net
//!   for a depth that takes far longer than expected.
//!
//! The soft limit is a share of the remaining time (1/30th, or 1/moves-to-go when the time
//! control says how many moves are left) plus most of the increment. The hard limit is a few
//! times the soft one. Neither may exceed half the clock, and both leave a safety margin
//! (`Move Overhead`) for the time the GUI and operating system take to pass messages.
//!
//! ## Adapting the soft limit (M5)
//!
//! Some moves need more thought than others, and the search itself shows which. After each
//! depth, the soft limit is scaled by two signals:
//!
//! - **Best-move stability.** If the best move has stayed the same for several depths in a
//!   row, deeper searches are unlikely to change it: stop sooner (down to 75% of the soft
//!   limit after four). If it has just changed, the search hasn't settled: think longer (up to
//!   180%).
//! - **A falling score.** If the score dropped since the last depth, the search has found a
//!   problem and may still find the answer to it: think longer, 1% more per centipawn it fell,
//!   up to twice as long.
//!
//! The scaled limit never passes the hard limit, so the clock is as safe as before. Time saved
//! on easy moves isn't lost: each move's budget is a share of what remains, so it is spent on
//! later moves. A fixed `movetime` is left alone (the GUI asked for that time exactly), and so
//! is the time after a `ponderhit`.
//!
//! Against the same engine with a fixed soft limit: +33.5 ± 14.3 Elo after 1,062 games at
//! 8+0.08 (cjus/chessengine#72). At the long time control, 40+0.4, switching it off loses
//! 13.8 ± 8.6 Elo after 2,599 games (cjus/chessengine#76): smaller, but the gain holds.
//!
//! See the Chess Programming Wiki, [Time Management](https://www.chessprogramming.org/Time_Management).

use std::time::Duration;

use cb2026_chess::Color;

use crate::Limits;

// ANCHOR: params
/// The time manager's tunable numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeParams {
    /// When the time control doesn't say, plan as if this many moves remain.
    pub moves_to_go: u32,
    /// Percentage of the increment added to each move's budget.
    pub increment_percent: u32,
    /// The hard limit is this many times the soft limit.
    pub hard_factor: u32,
    /// Time held back for communication delays (the UCI option `Move Overhead`).
    pub move_overhead: Duration,
    /// Scale the soft limit by how long the best move has stayed the same (`set tmstability`).
    pub best_move_stability: bool,
    /// The soft limit's scale, in percent, when the best move has stayed the same for 0, 1, 2,
    /// 3, and 4 or more depths in a row.
    pub stability_percent: [u32; 5],
    /// Think longer when the score falls from one depth to the next (`set tmscore`)...
    pub falling_score: bool,
    /// ...by this many percent per centipawn it fell...
    pub falling_score_percent_per_cp: u32,
    /// ...counting at most this many centipawns.
    pub falling_score_max_cp: u32,
}

impl Default for TimeParams {
    fn default() -> TimeParams {
        TimeParams {
            moves_to_go: 30,
            increment_percent: 75,
            hard_factor: 3,
            move_overhead: Duration::from_millis(30),
            best_move_stability: true,
            stability_percent: [180, 140, 110, 90, 75],
            falling_score: true,
            falling_score_percent_per_cp: 1,
            falling_score_max_cp: 100,
        }
    }
}
// ANCHOR_END: params

/// How long the search may run, measured from its start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimeBudget {
    /// Don't start another iteration after this.
    pub soft: Option<Duration>,
    /// Stop at once after this.
    pub hard: Option<Duration>,
    /// Whether the soft limit may adapt to the search (see [`adapted_soft`]): with a clock,
    /// not with a fixed `movetime`.
    pub adaptive: bool,
}

// ANCHOR: budget
/// The time budget for a search with `limits`, with `side` to move.
pub fn budget(limits: &Limits, side: Color, params: &TimeParams) -> TimeBudget {
    if let Some(movetime) = limits.movetime {
        let usable = movetime.saturating_sub(params.move_overhead);
        return TimeBudget {
            soft: Some(usable),
            hard: Some(usable),
            adaptive: false,
        };
    }
    let Some(remaining) = limits.time[side.index()] else {
        return TimeBudget::default();
    };
    let remaining = remaining.saturating_sub(params.move_overhead);
    let increment = limits.increment[side.index()].unwrap_or(Duration::ZERO);
    let moves_left = limits
        .moves_to_go
        .unwrap_or(params.moves_to_go)
        .clamp(1, params.moves_to_go);
    let ceiling = remaining / 2;
    let soft = remaining / moves_left + increment * params.increment_percent / 100;
    TimeBudget {
        soft: Some(soft.min(ceiling)),
        hard: Some((soft * params.hard_factor).min(ceiling)),
        adaptive: true,
    }
}
// ANCHOR_END: budget

// ANCHOR: adapt
/// The soft limit after a depth, adapted to how the search is going: `stable` is how many
/// depths in a row the best move has stayed the same, `fall` how many centipawns the score
/// dropped since the last depth (negative if it rose). Never past the hard limit.
pub fn adapted_soft(
    budget: &TimeBudget,
    stable: u32,
    fall: i32,
    params: &TimeParams,
) -> Option<Duration> {
    let soft = budget.soft?;
    if !budget.adaptive {
        return Some(soft);
    }
    let mut percent = 100;
    if params.best_move_stability {
        let row = (stable as usize).min(params.stability_percent.len() - 1);
        percent = percent * params.stability_percent[row] / 100;
    }
    if params.falling_score && fall > 0 {
        let fall = (fall as u32).min(params.falling_score_max_cp);
        percent = percent * (100 + fall * params.falling_score_percent_per_cp) / 100;
    }
    let adapted = soft * percent / 100;
    Some(budget.hard.map_or(adapted, |hard| adapted.min(hard)))
}
// ANCHOR_END: adapt

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults with both of the soft limit's scales switched on.
    fn adaptive() -> TimeParams {
        TimeParams {
            best_move_stability: true,
            falling_score: true,
            ..TimeParams::default()
        }
    }

    fn clock(seconds: u64, increment_ms: u64) -> Limits {
        Limits {
            time: [Some(Duration::from_secs(seconds)), None],
            increment: [Some(Duration::from_millis(increment_ms)), None],
            ..Limits::default()
        }
    }

    #[test]
    fn clock_budget() {
        let params = TimeParams::default();
        let b = budget(&clock(60, 1000), Color::White, &params);
        // (60 s − 30 ms) / 30 + 750 ms = 2749 ms, hard limit three times that.
        assert_eq!(b.soft, Some(Duration::from_millis(2749)));
        assert_eq!(b.hard, Some(Duration::from_millis(8247)));
        assert_eq!(
            budget(&clock(60, 1000), Color::Black, &params),
            TimeBudget::default()
        );
    }

    #[test]
    fn never_more_than_half_the_clock() {
        let params = TimeParams::default();
        // Very little time left but a big increment.
        let b = budget(&clock(1, 5000), Color::White, &params);
        assert_eq!(b.hard, Some(Duration::from_millis(485)));
        assert!(b.soft <= b.hard);
    }

    #[test]
    fn moves_to_go_and_movetime() {
        let params = TimeParams::default();
        let limits = Limits {
            moves_to_go: Some(10),
            ..clock(60, 0)
        };
        assert_eq!(
            budget(&limits, Color::White, &params).soft,
            Some(Duration::from_millis(5997))
        );
        let fixed = Limits {
            movetime: Some(Duration::from_millis(500)),
            ..Limits::default()
        };
        let b = budget(&fixed, Color::White, &params);
        assert_eq!(b.soft, Some(Duration::from_millis(470)));
        assert_eq!(b.hard, b.soft);
        assert!(!b.adaptive);
    }

    #[test]
    fn a_stable_best_move_stops_sooner_and_a_new_one_thinks_longer() {
        let params = adaptive();
        let b = budget(&clock(60, 0), Color::White, &params);
        let soft = b.soft.unwrap();
        // Just changed: 180%. Four or more depths the same: 75%.
        assert_eq!(adapted_soft(&b, 0, 0, &params), Some(soft * 180 / 100));
        assert_eq!(adapted_soft(&b, 4, 0, &params), Some(soft * 75 / 100));
        assert_eq!(adapted_soft(&b, 9, 0, &params), Some(soft * 75 / 100));
        // Switched off: the soft limit as it was.
        let off = TimeParams {
            best_move_stability: false,
            ..adaptive()
        };
        assert_eq!(adapted_soft(&b, 0, 0, &off), Some(soft));
    }

    #[test]
    fn a_falling_score_thinks_longer_up_to_a_cap() {
        let params = TimeParams {
            best_move_stability: false,
            ..adaptive()
        };
        let b = budget(&clock(60, 0), Color::White, &params);
        let soft = b.soft.unwrap();
        // 30 centipawns down: 130%. A rising score changes nothing. 500 down counts as 100.
        assert_eq!(adapted_soft(&b, 0, 30, &params), Some(soft * 130 / 100));
        assert_eq!(adapted_soft(&b, 0, -30, &params), Some(soft));
        assert_eq!(adapted_soft(&b, 0, 500, &params), Some(soft * 2));
    }

    #[test]
    fn the_adapted_limit_never_passes_the_hard_limit_or_a_fixed_movetime() {
        let params = adaptive();
        // With a big increment the soft limit is already near the hard one (half the clock).
        let b = budget(&clock(1, 5000), Color::White, &params);
        assert_eq!(adapted_soft(&b, 0, 100, &params), b.hard);
        let fixed = Limits {
            movetime: Some(Duration::from_millis(500)),
            ..Limits::default()
        };
        let b = budget(&fixed, Color::White, &params);
        assert_eq!(adapted_soft(&b, 9, 0, &params), b.soft);
        assert_eq!(adapted_soft(&TimeBudget::default(), 0, 0, &params), None);
    }
}
