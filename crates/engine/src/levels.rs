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
//! Those are still a strong player's mistakes. Every candidate comes from a search that sees
//! the short tactics, so a weak level seldom hangs a piece or misses one hung, and people find
//! it far stronger than a person of its rating (Solrac Chess AI #113). Counted as
//! Lichess counts them (the `mistakes` tool in `cb2026-tools`), people blunder 2 to 4 times in
//! 40 moves across almost the whole range of ratings, and the levels did far less: 0.7 at the
//! level once rated 1025 on CCRL, where people rated about 1000 blunder 3.4 times.
//!
//! People's worst mistakes come from **their horizon**: they don't see the reply, so a piece
//! is left where it can be taken, or they see the reply but not the move after it, so a fork
//! or a pin comes as a surprise. So a weaker level chooses many of its moves **with a person's
//! horizon** ([`LevelSettings::horizon_chance`], [`horizon`]): a search of one ply, the move
//! alone, or from [`TWO_PLY_HORIZON_FROM`] two, the move and the reply
//! ([`LevelSettings::horizon_depth`]), with no quiescence search and nothing pruned
//! ([`Techniques::horizon`](crate::search::Techniques::horizon)), then the usual choice among
//! its best moves. Such a move is often fine: it is a blunder only when there was a tactic to
//! miss, which is when people blunder too.
//!
//! **The levels are on a human scale** (Carlos's choice, Solrac Chess AI #113): level 0 is a
//! Lichess rapid rating of 600, and each level 100 more, to 2500 at level 19 ([`RATINGS`],
//! [`RATING_SCALE`]). Each level's chance of a person's horizon ([`HORIZON_CHANCES`]) is fitted
//! so that it blunders as often as people of its rating do ([`PEOPLE_BLUNDERS`], counted in
//! Lichess's own games). It is an estimate from blunders alone: a level's other moves are an
//! engine's, and only games against people would rate it for sure. A level that blunders like
//! a person loses to engines far more than a person of its rating would, since engines punish
//! every blunder, so the levels' old ratings against engines (CCRL Blitz, measured at 40+0.4 on
//! the test Mac, cjus/chessengine#106; chapter 19) no longer hold, and are kept only in the
//! book and the Elo ledger.
//!
//! See the Chess Programming Wiki, [Strength](https://www.chessprogramming.org/Strength) and
//! [Playing Strength](https://www.chessprogramming.org/Playing_Strength).

use cb2026_chess::Move;

/// The full-strength level: no limits.
pub const FULL_STRENGTH: u32 = 20;
/// The lowest and highest `UCI_Elo` (and CNP `strength.elo`) accepted: level 0's and level 19's
/// ratings in [`RATINGS`]. On the CCRL scale it was 650 to 2450 after the calibration at 40+0.4,
/// 800 to 2600 after the one at 8+0.08 (cjus/chessengine#106), and 1000 to 2400 before any.
pub const ELO_RANGE: (u32, u32) = (600, 2500);

/// What [`RATINGS`] mean, as CNP's `engine/levels` names it.
pub const RATING_SCALE: &str = "Lichess rapid, estimated from how often people blunder";

/// From this level on, a person's horizon sees the reply too (two plies); below it, only the
/// move (one ply). Level 8 is a Lichess rating of 1400.
pub const TWO_PLY_HORIZON_FROM: f64 = 8.0;

// ANCHOR: ratings
/// Each level's rating, 0 to [`FULL_STRENGTH`], on [`RATING_SCALE`]: the Lichess rapid rating
/// of the people who blunder as often as the level does ([`PEOPLE_BLUNDERS`]), 600 at level 0
/// and 100 more for each level, to 2500 at level 19. Full strength makes no blunders to compare,
/// and plays far above any person: its 3000 only stands above the scale, and is never asked for
/// ([`ELO_RANGE`] stops at level 19's).
pub const RATINGS: [u32; FULL_STRENGTH as usize + 1] = [
    600, 700, 800, 900, 1000, 1100, 1200, 1300, 1400, 1500, 1600, 1700, 1800, 1900, 2000, 2100,
    2200, 2300, 2400, 2500, 3000,
];
// ANCHOR_END: ratings

/// How often people at each level's rating (0 to 19, [`RATINGS`]) blunder, per 40 moves of one
/// player, as Lichess's analysis counts it: 60,000 rated rapid games of January 2025 that carry
/// Lichess's evaluations (<https://database.lichess.org/>), counted by `mistakes --people` in
/// `cb2026-tools`. Each is the average of its 100-point band (600 to 699 for level 0) and the
/// bands beside it. What the levels are fitted to.
pub const PEOPLE_BLUNDERS: [f64; FULL_STRENGTH as usize] = [
    4.54, 4.31, 4.06, 3.67, 3.43, 3.18, 2.99, 2.86, 2.71, 2.59, 2.41, 2.25, 2.11, 1.99, 1.83, 1.66,
    1.48, 1.25, 1.05, 0.90,
];

// ANCHOR: horizon_chances
/// The chance at each level, 0 to 19, that a move is chosen with a person's horizon: fitted with
/// the `mistakes` tool so that each level blunders as often as [`PEOPLE_BLUNDERS`] says (see the
/// module docs). It rises at [`TWO_PLY_HORIZON_FROM`], where a horizon that sees the reply makes
/// fewer blunders per move. Full strength: never.
///
/// The fit, in three rounds of 40 self-play games a level, each move judged at 100,000 nodes:
/// a level blunders about as often as its usual search does (2.8 times in 40 moves at level 0
/// with the horizon off, none by level 18), plus the chance times what a horizon move adds
/// (about 5 blunders in 40 with one ply, 3 to 4 with two). Each round set each level's chance
/// from how far the last missed, and the last is smoothed into a straight line through levels 0
/// to 7 and another through 8 to 19, since one level's 40 games carry about ± 0.25 blunders of
/// noise (chapter 19 has the rounds).
pub const HORIZON_CHANCES: [f64; FULL_STRENGTH as usize] = [
    0.78, 0.73, 0.68, 0.64, 0.59, 0.55, 0.50, 0.46, 0.70, 0.70, 0.69, 0.65, 0.62, 0.58, 0.55, 0.51,
    0.48, 0.44, 0.41, 0.37,
];
// ANCHOR_END: horizon_chances

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
    /// The chance, 0 to 1, that a move is chosen with a person's horizon ([`horizon`]).
    pub horizon_chance: f64,
    /// How many plies a person's horizon sees: 1, the move alone, or 2, the reply too.
    pub horizon_depth: u32,
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
            horizon_chance: 0.0,
            horizon_depth: 0,
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
        // A whole level's: the depth can't be a fraction, and the chance jumps where the depth
        // changes, so between two levels the lower one's.
        horizon_chance: HORIZON_CHANCES[(level.floor() as usize).min(HORIZON_CHANCES.len() - 1)],
        horizon_depth: if level.floor() < TWO_PLY_HORIZON_FROM {
            1
        } else {
            2
        },
    }
}
// ANCHOR_END: settings

// ANCHOR: horizon
/// Whether this move is chosen with a person's horizon, using `random` (any 64-bit number) as
/// the dice: the plies to search if so, `None` for the level's usual search.
pub fn horizon(settings: &LevelSettings, random: u64) -> Option<u32> {
    // A number in [0, 1).
    let roll = (random >> 11) as f64 / (1u64 << 53) as f64;
    (roll < settings.horizon_chance).then_some(settings.horizon_depth)
}
// ANCHOR_END: horizon

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
    fn weaker_levels_more_often_look_only_as_far_as_a_person() {
        assert_eq!(settings(FULL_STRENGTH).horizon_chance, 0.0);
        assert_eq!(settings(0).horizon_chance, HORIZON_CHANCES[0]);
        // Within each horizon, a stronger level looks that way no more often.
        let first_two_ply = TWO_PLY_HORIZON_FROM as u32;
        for level in (1..FULL_STRENGTH).filter(|&level| level != first_two_ply) {
            assert!(settings(level).horizon_chance <= settings(level - 1).horizon_chance);
        }
        // Between two levels, the lower one's horizon.
        assert_eq!(settings_at(7.5).horizon_chance, settings(7).horizon_chance);
        assert_eq!(settings_at(7.5).horizon_depth, 1);
        // People blunder less as their rating rises.
        assert!(PEOPLE_BLUNDERS.windows(2).all(|pair| pair[0] > pair[1]));
        // Weak levels don't see the reply; stronger ones see it, but not the move after.
        assert_eq!(settings(0).horizon_depth, 1);
        assert_eq!(settings(7).horizon_depth, 1);
        assert_eq!(settings(8).horizon_depth, 2);
        assert_eq!(settings(19).horizon_depth, 2);
        // The dice: as often as the chance says.
        let level = settings(0);
        let rolls = 10_000u64;
        let looked = (0..rolls)
            .filter(|i| horizon(&level, i.wrapping_mul(0x9E37_79B9_7F4A_7C15)).is_some())
            .count();
        let share = looked as f64 / rolls as f64;
        assert!((share - HORIZON_CHANCES[0]).abs() < 0.02, "{share}");
        assert_eq!(horizon(&settings(FULL_STRENGTH), 0), None);
    }

    #[test]
    fn elo_maps_onto_the_ratings() {
        // Each level is stronger than the one below it, 100 points a level.
        assert!(RATINGS.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(RATINGS[0], 600);
        assert_eq!(RATINGS[19], 2500);
        // A level's own rating maps onto it exactly, and other Elos onto in-between strengths.
        assert_eq!(level_for_elo(RATINGS[10]), 10.0);
        assert_eq!(level_for_elo(1050), 4.5);
        // The range's ends are levels 0 and 19, and outside it, the nearest end.
        assert_eq!(level_for_elo(ELO_RANGE.0), 0.0);
        assert_eq!(level_for_elo(ELO_RANGE.1), 19.0);
        assert_eq!(level_for_elo(400), 0.0);
        assert_eq!(level_for_elo(2800), 19.0);
        // And back: the rating of the strength for an Elo is that Elo.
        for elo in (ELO_RANGE.0..=ELO_RANGE.1).step_by(50) {
            assert_eq!(rating(level_for_elo(elo)), elo, "{elo}");
        }
        assert_eq!(rating(f64::from(FULL_STRENGTH)), 3000);
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
