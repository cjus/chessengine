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
}

impl Default for TimeParams {
    fn default() -> TimeParams {
        TimeParams {
            moves_to_go: 30,
            increment_percent: 75,
            hard_factor: 3,
            move_overhead: Duration::from_millis(30),
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
}

// ANCHOR: budget
/// The time budget for a search with `limits`, with `side` to move.
pub fn budget(limits: &Limits, side: Color, params: &TimeParams) -> TimeBudget {
    if let Some(movetime) = limits.movetime {
        let usable = movetime.saturating_sub(params.move_overhead);
        return TimeBudget {
            soft: Some(usable),
            hard: Some(usable),
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
    }
}
// ANCHOR_END: budget

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
