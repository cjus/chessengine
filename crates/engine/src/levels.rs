//! Strength levels: playing weaker on purpose, believably.
//!
//! A full-strength engine is no fun to play for most people, so engines offer levels: the UCI
//! `Skill Level` (0–20) and `UCI_LimitStrength` with `UCI_Elo`. The obvious way to weaken an
//! engine is to search less, and cb2026 does limit the nodes per move. But that alone gives
//! odd play: even a shallow search finds every one-move tactic, and its mistakes are subtle
//! long-term ones a human of that strength wouldn't make, while it never misses a hanging
//! piece. People don't play like that.
//!
//! So weaker levels also **choose among the top few moves** (found with MultiPV), with
//! controlled randomness: a move scoring close to the best is often played, one a pawn worse
//! sometimes, a blunder that loses a rook practically never. The engine then makes the kind of
//! mistakes a player of that level makes: inaccuracies, more of them at lower levels.
//!
//! The levels are **calibrated**, as the PRD requires: on 2026-10-18 each level played 800 games
//! against the four reference engines nearest its strength, engines with known ratings on the
//! CCRL Blitz list (cjus/chessengine#103, `scripts/mac-levels.sh`), and [`RATINGS`] records the
//! result. `UCI_Elo` maps onto those ratings, between two levels where it falls between them:
//! the settings are smooth in the level, so [`settings_at`] takes a fractional one. The ratings
//! are on the CCRL scale, which isn't the same as FIDE or Lichess ratings, and they were
//! measured at 8+0.08: the levels limit nodes, not time, so against engines given more time
//! they would rate lower (see chapter 19).
//!
//! See the Chess Programming Wiki, [Strength](https://www.chessprogramming.org/Strength) and
//! [Playing Strength](https://www.chessprogramming.org/Playing_Strength).

use cb2026_chess::Move;

/// The full-strength level: no limits.
pub const FULL_STRENGTH: u32 = 20;
/// The lowest and highest `UCI_Elo` accepted.
pub const ELO_RANGE: (u32, u32) = (1000, 2400);

// ANCHOR: ratings
/// The measured rating of each level, 0 to [`FULL_STRENGTH`], on the CCRL Blitz scale. Levels
/// 0–19: the calibration of 2026-10-18 on the test Mac (cjus/chessengine#103), 800 games each
/// at 8+0.08 against the four reference engines nearest it, ± 22 to 41 (level 0, weaker than
/// every reference engine, has the widest margin). Full strength: that day's gauntlet of
/// `main`, 2714 ± 10 (cjus/chessengine#36).
pub const RATINGS: [u32; FULL_STRENGTH as usize + 1] = [
    752, 823, 851, 927, 999, 1052, 1131, 1242, 1307, 1414, 1534, 1642, 1783, 1924, 2106, 2240,
    2368, 2475, 2533, 2599, 2714,
];
// ANCHOR_END: ratings

/// How a level plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelSettings {
    /// Nodes searched per move (`None` at full strength).
    pub nodes: Option<u64>,
    /// How many of the best moves are candidates (MultiPV lines searched).
    pub candidates: usize,
    /// Moves more than this many centipawns worse than the best are never chosen.
    pub margin: i32,
    /// How readily worse moves are chosen: a move `temperature` centipawns worse than the
    /// best is chosen `e` (2.7) times less often.
    pub temperature: f64,
}

/// The settings for `level` (0 to [`FULL_STRENGTH`]).
pub fn settings(level: u32) -> LevelSettings {
    settings_at(f64::from(level))
}

// ANCHOR: settings
/// The settings at a strength `level` from 0 to [`FULL_STRENGTH`], which can fall between two
/// levels: `UCI_Elo` maps onto in-between strengths.
pub fn settings_at(level: f64) -> LevelSettings {
    if level >= f64::from(FULL_STRENGTH) {
        return LevelSettings {
            nodes: None,
            candidates: 1,
            margin: 0,
            temperature: 0.0,
        };
    }
    let level = level.max(0.0);
    // 1.5 to the whole levels, then to the fraction, so that a whole level's node count is exact.
    let growth = 1.5f64.powi(level.floor() as i32) * 1.5f64.powf(level.fract());
    LevelSettings {
        // 400 nodes at level 0, growing by half each level: about 890,000 at level 19.
        nodes: Some((400.0 * growth) as u64),
        candidates: 4,
        margin: (300.0 - 15.0 * level).round() as i32,
        temperature: 120.0 - 6.0 * level,
    }
}
// ANCHOR_END: settings

// ANCHOR: level_for_elo
/// The strength for a `UCI_Elo` setting (clamped to [`ELO_RANGE`]): the level with that measured
/// rating in [`RATINGS`], interpolated linearly between the two levels it falls between.
pub fn level_for_elo(elo: u32) -> f64 {
    let (low, high) = ELO_RANGE;
    let elo = elo.clamp(low, high);
    // The first level rated at least `elo`; there is one, since full strength is above the range.
    let above = RATINGS
        .iter()
        .position(|&r| r >= elo)
        .unwrap_or(RATINGS.len() - 1);
    if above == 0 {
        return 0.0;
    }
    let (below_rating, above_rating) = (RATINGS[above - 1], RATINGS[above]);
    (above - 1) as f64 + f64::from(elo - below_rating) / f64::from(above_rating - below_rating)
}
// ANCHOR_END: level_for_elo

/// The rating of a strength `level`, interpolated in [`RATINGS`] (the inverse of
/// [`level_for_elo`]).
pub fn rating(level: f64) -> u32 {
    let level = level.clamp(0.0, f64::from(FULL_STRENGTH));
    let below = (level.floor() as usize).min(FULL_STRENGTH as usize - 1);
    let fraction = level - below as f64;
    let (low, high) = (f64::from(RATINGS[below]), f64::from(RATINGS[below + 1]));
    (low + fraction * (high - low)).round() as u32
}

// ANCHOR: choose
/// Chooses a move from `candidates` (move and score, best first) for a level, using `random`
/// (any 64-bit number) as the dice. Each move within the margin is weighted by
/// `exp(-(best - score) / temperature)`.
pub fn choose(candidates: &[(Move, i32)], settings: &LevelSettings, random: u64) -> Option<Move> {
    let &(first, best) = candidates.first()?;
    if settings.temperature <= 0.0 {
        return Some(first);
    }
    let weights: Vec<(Move, f64)> = candidates
        .iter()
        .filter(|&&(_, score)| best - score <= settings.margin)
        .map(|&(mv, score)| {
            (
                mv,
                (-(f64::from(best - score)) / settings.temperature).exp(),
            )
        })
        .collect();
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    // A number in [0, total).
    let mut roll = (random >> 11) as f64 / (1u64 << 53) as f64 * total;
    for &(mv, weight) in &weights {
        if roll < weight {
            return Some(mv);
        }
        roll -= weight;
    }
    Some(first)
}
// ANCHOR_END: choose

#[cfg(test)]
mod tests {
    use super::*;
    use cb2026_chess::Position;

    #[test]
    fn levels_get_stronger() {
        assert_eq!(settings(FULL_STRENGTH).nodes, None);
        assert_eq!(settings(0).nodes, Some(400));
        for level in 1..FULL_STRENGTH {
            let (weak, strong) = (settings(level - 1), settings(level));
            assert!(strong.nodes > weak.nodes);
            assert!(strong.margin < weak.margin);
            assert!(strong.temperature < weak.temperature);
        }
    }

    #[test]
    fn whole_levels_keep_their_settings() {
        assert_eq!(settings(5).nodes, Some(3037));
        assert_eq!(settings(10).nodes, Some(23066));
        assert_eq!(settings(19).nodes, Some(886_735));
        assert_eq!(settings(10).margin, 150);
        // In between, every setting lies between the two levels'.
        let (low, mid, high) = (settings(9), settings_at(9.5), settings(10));
        assert!(low.nodes < mid.nodes && mid.nodes < high.nodes);
        assert!(low.margin > mid.margin && mid.margin > high.margin);
        assert!(low.temperature > mid.temperature && mid.temperature > high.temperature);
    }

    #[test]
    fn elo_maps_onto_the_measured_ratings() {
        // Each level is stronger than the one below it.
        assert!(RATINGS.windows(2).all(|pair| pair[0] < pair[1]));
        // A level's own rating maps onto it exactly, and the range's ends onto in-between
        // strengths: 1000 just above level 4 (999), 2400 between levels 16 (2368) and 17 (2475).
        assert_eq!(level_for_elo(RATINGS[10]), 10.0);
        assert!(level_for_elo(1000) > 4.0 && level_for_elo(1000) < 4.1);
        assert!(level_for_elo(2400) > 16.0 && level_for_elo(2400) < 17.0);
        // Outside the range, the nearest end.
        assert_eq!(level_for_elo(500), level_for_elo(1000));
        assert_eq!(level_for_elo(3000), level_for_elo(2400));
        // And back: the rating of the strength for an Elo is that Elo.
        for elo in (ELO_RANGE.0..=ELO_RANGE.1).step_by(50) {
            assert_eq!(rating(level_for_elo(elo)), elo, "{elo}");
        }
        assert_eq!(rating(f64::from(FULL_STRENGTH)), 2714);
    }

    #[test]
    fn choices_favour_good_moves_and_never_blunders() {
        let pos = Position::startpos();
        let mv = |uci: &str| pos.parse_uci_move(uci).unwrap();
        let candidates = [
            (mv("e2e4"), 30),
            (mv("d2d4"), 25),
            (mv("g1f3"), -20),
            (mv("f2f3"), -500),
        ];
        let level = settings(5);
        let mut counts = [0; 4];
        for i in 0..2000u64 {
            let random = i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let chosen = choose(&candidates, &level, random).unwrap();
            counts[candidates.iter().position(|c| c.0 == chosen).unwrap()] += 1;
        }
        assert!(counts[0] > counts[1] && counts[1] > counts[2], "{counts:?}");
        assert!(counts[2] > 0, "{counts:?}");
        assert_eq!(counts[3], 0, "a 5-pawn blunder is outside the margin");
        // Full strength always plays the best move.
        assert_eq!(
            choose(&candidates, &settings(FULL_STRENGTH), 12345),
            Some(mv("e2e4"))
        );
    }
}
