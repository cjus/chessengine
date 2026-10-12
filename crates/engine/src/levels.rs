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
//! The numbers below are starting values. The PRD requires them to be **calibrated**: each level
//! is played against reference engines with known ratings (CCRL), and the `UCI_Elo` mapping is
//! fitted to the results. Until then the Elo figures are estimates. They are on the CCRL scale,
//! which isn't the same as FIDE or Lichess ratings.
//!
//! See the Chess Programming Wiki, [Strength](https://www.chessprogramming.org/Strength) and
//! [Playing Strength](https://www.chessprogramming.org/Playing_Strength).

use cb2026_chess::Move;

/// The full-strength level: no limits.
pub const FULL_STRENGTH: u32 = 20;
/// The lowest and highest `UCI_Elo` accepted.
pub const ELO_RANGE: (u32, u32) = (1000, 2400);

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

// ANCHOR: settings
/// The settings for `level` (0 to [`FULL_STRENGTH`]).
pub fn settings(level: u32) -> LevelSettings {
    if level >= FULL_STRENGTH {
        return LevelSettings {
            nodes: None,
            candidates: 1,
            margin: 0,
            temperature: 0.0,
        };
    }
    let level = level as i32;
    LevelSettings {
        // 400 nodes at level 0, growing by half each level: about 890,000 at level 19.
        nodes: Some((400.0 * 1.5f64.powi(level)) as u64),
        candidates: 4,
        margin: 300 - 15 * level,
        temperature: f64::from(120 - 6 * level),
    }
}
// ANCHOR_END: settings

/// The level for a `UCI_Elo` setting: a straight line from 1000 (level 0) to 2400 (level 19).
/// Provisional, until the levels are calibrated.
pub fn level_for_elo(elo: u32) -> u32 {
    let (low, high) = ELO_RANGE;
    let elo = elo.clamp(low, high);
    ((elo - low) * (FULL_STRENGTH - 1) + (high - low) / 2) / (high - low)
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
    fn elo_maps_onto_levels() {
        assert_eq!(level_for_elo(500), 0);
        assert_eq!(level_for_elo(1000), 0);
        assert_eq!(level_for_elo(2400), 19);
        assert_eq!(level_for_elo(3000), 19);
        assert!(level_for_elo(1700) > 8 && level_for_elo(1700) < 11);
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
