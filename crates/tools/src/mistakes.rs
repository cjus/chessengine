//! How often a strength level makes mistakes, counted as Lichess counts them, to compare
//! cb2026's weaker levels with people (cjus/chessgui#113, [`cb2026_engine::levels`]).
//!
//! A level plays itself from varied openings, and a full-strength search with a fixed node
//! count judges every move by how much it lowered the mover's **winning chances**, from −1
//! (lost) to 1 (won): a drop of 0.1 is an inaccuracy, 0.2 a mistake and 0.3 a blunder. Winning
//! chances come from centipawns, capped at ±1000, through a curve fitted to Lichess's games,
//! `2 / (1 + e^(−0.00368208 · cp)) − 1`; a mate in n counts as (21 − min(10, n)) × 100
//! centipawns. The formula, the cap and the thresholds are Lichess's
//! (<https://lichess.org/page/accuracy>; lila's `Advice.scala` and `winningChances.ts`,
//! AGPL-3.0: the numbers only, none of its code), so the counts compare with what Lichess's
//! analysis says about people's games.
//!
//! **People** are counted by the same code, from Lichess's own games ([`count_people`]): the
//! monthly files at <https://database.lichess.org/> carry the evaluation Lichess's analysis
//! gave each move (`[%eval 0.17]`) in the games someone had analysed, about one in twelve.
//! [`Tally::per_game`] gives counts for 40 moves of one player, for people and levels alike.
//!
//! ## Pitfalls
//!
//! - **The judge's own horizon.** A search of a few hundred thousand nodes misjudges a few
//!   moves; the counts are for comparing levels, and with people's counts roughly, not to the
//!   decimal.
//! - **Decided games.** Moves in a lost or won position barely move the winning chances, as on
//!   Lichess, so they are seldom counted as mistakes.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use cb2026_chess::Position;
use cb2026_chess::game::Game;
use cb2026_engine::{Engine, Limits, Score};

use crate::selfplay::{DatagenSettings, starting_position};

/// Lichess's curve from centipawns to winning chances: its slope.
pub const WINNING_CHANCES_SLOPE: f64 = 0.00368208;
/// The centipawns Lichess caps an evaluation at before the curve.
pub const CENTIPAWN_CAP: i32 = 1000;
/// The drop in winning chances that makes a move an inaccuracy, a mistake and a blunder.
pub const THRESHOLDS: [f64; 3] = [0.1, 0.2, 0.3];
/// A game's length for [`Tally::per_game`]: 40 moves of each player.
pub const MOVES_PER_GAME: f64 = 40.0;
/// Games stop after this many plies, as a draw.
pub const MAX_PLIES: usize = 200;
/// The evaluation of the starting position that Lichess's games begin from, in centipawns
/// from White's side: the first move's drop is measured from it.
pub const START_EVALUATION: i32 = 20;
/// Lichess's speeds, by a game's estimated length (the base time plus 40 increments), in
/// seconds: below each bound, the speed beside it; from the last bound on, classical.
pub const SPEEDS: [(u64, &str); 4] = [
    (30, "ultrabullet"),
    (180, "bullet"),
    (480, "blitz"),
    (1500, "rapid"),
];

/// The mover's winning chances, −1 to 1, for an evaluation from its side.
pub fn winning_chances(score: Score) -> f64 {
    let cp = match score {
        Score::Centipawns(cp) => cp.clamp(-CENTIPAWN_CAP, CENTIPAWN_CAP),
        Score::Mate(moves) => (21 - moves.abs().min(10)) * 100 * moves.signum(),
    };
    2.0 / (1.0 + (-WINNING_CHANCES_SLOPE * f64::from(cp)).exp()) - 1.0
}

/// How bad a move was: 0 for none of these, 1 an inaccuracy, 2 a mistake, 3 a blunder, by the
/// drop in the mover's winning chances.
pub fn classify(drop: f64) -> usize {
    THRESHOLDS.iter().filter(|&&at| drop >= at).count()
}

/// A level's moves, and how many of them were each kind of mistake.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    /// Moves judged.
    pub moves: usize,
    /// Inaccuracies, mistakes and blunders (each counted once, as its worst kind).
    pub errors: [usize; 3],
}

impl Tally {
    /// Adds a move whose kind ([`classify`]) is `kind`.
    pub fn add(&mut self, kind: usize) {
        self.moves += 1;
        if kind > 0 {
            self.errors[kind - 1] += 1;
        }
    }

    /// Adds another tally.
    pub fn merge(&mut self, other: &Tally) {
        self.moves += other.moves;
        for (mine, theirs) in self.errors.iter_mut().zip(other.errors) {
            *mine += theirs;
        }
    }

    /// Inaccuracies, mistakes and blunders per [`MOVES_PER_GAME`] moves: a game's worth for
    /// one player.
    pub fn per_game(&self) -> [f64; 3] {
        self.errors
            .map(|count| count as f64 * MOVES_PER_GAME / (self.moves.max(1)) as f64)
    }
}

/// How to measure.
#[derive(Clone, Debug)]
pub struct MistakeSettings {
    /// The strength level, 0 to 20 (the UCI `Skill Level`).
    pub level: u32,
    /// Games to play.
    pub games: usize,
    /// Nodes for the full-strength judge, per position.
    pub judge_nodes: u64,
    /// Games played at the same time.
    pub threads: usize,
    /// Where the games start: openings and random plies, as for self-play data.
    pub openings: DatagenSettings,
}

/// Plays one game of `player` against itself from `start`, and judges its moves with `judge`.
pub fn play_game(player: &mut Engine, judge: &mut Engine, start: &Position, nodes: u64) -> Tally {
    player.new_game();
    judge.new_game();
    let stop = AtomicBool::new(false);
    let judging = Limits {
        nodes: Some(nodes),
        ..Limits::default()
    };
    let evaluate = |judge: &mut Engine, game: &Game| {
        let mut score = Score::Centipawns(0);
        let history = game.history_hashes();
        judge.search(game.position(), &history, &judging, &stop, &mut |info| {
            score = info.score
        });
        score
    };
    let mut game = Game::from_position(start.clone());
    let mut tally = Tally::default();
    let mut before = evaluate(judge, &game);
    while game.outcome().is_none() && game.moves().len() < MAX_PLIES {
        let history = game.history_hashes();
        let reply = player.search(
            game.position(),
            &history,
            &Limits::default(),
            &stop,
            &mut |_| {},
        );
        let Some(mv) = reply.best_move else {
            break;
        };
        game.play(mv).expect("the engine plays legal moves");
        // The position after, from the opponent's side: its chances are the mover's, negated.
        let after = if let Some(outcome) = game.outcome() {
            match outcome.winner() {
                // The mover just won: the opponent is mated.
                Some(_) => Score::Mate(-1),
                None => Score::Centipawns(0),
            }
        } else {
            evaluate(judge, &game)
        };
        let drop = winning_chances(before) + winning_chances(after);
        tally.add(classify(drop));
        before = after;
    }
    tally
}

/// Plays `settings.games` games and adds up the level's mistakes, on `settings.threads`
/// threads, calling `on_game` after each.
pub fn measure(settings: &MistakeSettings, on_game: &(dyn Fn(usize, &Tally) + Sync)) -> Tally {
    let next = AtomicUsize::new(0);
    let total = Mutex::new(Tally::default());
    thread::scope(|scope| {
        for _ in 0..settings.threads.max(1) {
            scope.spawn(|| {
                let mut player = Engine::new();
                player.set_hash_size(4);
                player.skill_level = settings.level;
                let mut judge = Engine::new();
                judge.set_hash_size(16);
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= settings.games {
                        break;
                    }
                    let start = starting_position(&settings.openings, index);
                    let tally = play_game(&mut player, &mut judge, &start, settings.judge_nodes);
                    let mut total = total.lock().unwrap_or_else(|e| e.into_inner());
                    total.merge(&tally);
                    on_game(index, &total);
                }
            });
        }
    });
    total.into_inner().unwrap_or_else(|e| e.into_inner())
}

/// Lichess's speed for a `TimeControl` header ("600+5"): `None` for a game without one.
pub fn speed(time_control: &str) -> Option<&'static str> {
    let (base, increment) = time_control.split_once('+')?;
    let length = base.parse::<u64>().ok()? + 40 * increment.parse::<u64>().ok()?;
    Some(
        SPEEDS
            .iter()
            .find(|&&(below, _)| length < below)
            .map_or("classical", |&(_, name)| name),
    )
}

/// An evaluation as Lichess's games write it, from White's side: pawns (`0.17`) or a mate
/// (`#-3`, Black mates in 3).
fn white_score(text: &str) -> Option<Score> {
    match text.strip_prefix('#') {
        Some(moves) => moves.parse().ok().map(Score::Mate),
        None => text
            .parse::<f64>()
            .ok()
            .map(|pawns| Score::Centipawns((pawns * 100.0).round() as i32)),
    }
}

/// Counts people's inaccuracies, mistakes and blunders in Lichess's games (PGN, read from
/// `games`), by speed and by the player's rating in bands of `band` points (1000 for 1000 to
/// 1099 with a band of 100), up to `max_games` games with evaluations. Returns the tallies
/// and how many evaluated games it read.
pub fn count_people(
    games: impl BufRead,
    max_games: usize,
    band: u32,
) -> (BTreeMap<(&'static str, u32), Tally>, usize) {
    let mut tallies = BTreeMap::new();
    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    let mut evaluated = 0;
    for line in games.lines() {
        let Ok(line) = line else {
            break;
        };
        if let Some(header) = line.strip_prefix('[') {
            if let Some((name, value)) = header.split_once(' ') {
                let value = value.trim_end_matches(']').trim_matches('"');
                headers.insert(name.to_string(), value.to_string());
            }
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        // The moves: one line, after the headers.
        let game = std::mem::take(&mut headers);
        if !line.contains("[%eval ") {
            continue;
        }
        let rating = |name: &str| game.get(name).and_then(|r| r.parse::<u32>().ok());
        let (Some(white), Some(black), Some(speed)) = (
            rating("WhiteElo"),
            rating("BlackElo"),
            game.get("TimeControl").and_then(|tc| speed(tc)),
        ) else {
            continue;
        };
        if game.get("Variant").is_some_and(|v| v != "Standard") {
            continue;
        }
        let mut before = winning_chances(Score::Centipawns(START_EVALUATION));
        for (ply, text) in line.split("[%eval ").skip(1).enumerate() {
            let Some(after) = text
                .split(']')
                .next()
                .and_then(white_score)
                .map(winning_chances)
            else {
                break;
            };
            // White's chances: Black's are the same, negated.
            let (player, drop) = if ply % 2 == 0 {
                (white, before - after)
            } else {
                (black, after - before)
            };
            tallies
                .entry((speed, player / band * band))
                .or_insert_with(Tally::default)
                .add(classify(drop));
            before = after;
        }
        evaluated += 1;
        if evaluated >= max_games {
            break;
        }
    }
    (tallies, evaluated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winning_chances_follow_lichess() {
        assert_eq!(winning_chances(Score::Centipawns(0)), 0.0);
        let pawn = winning_chances(Score::Centipawns(100));
        assert!((pawn - 0.1821).abs() < 1e-3, "{pawn}");
        // Capped at a thousand centipawns.
        assert_eq!(
            winning_chances(Score::Centipawns(5000)),
            winning_chances(Score::Centipawns(1000))
        );
        // A mate is nearly certain, and a mate soon a little more.
        assert!(winning_chances(Score::Mate(3)) > 0.99);
        assert!(winning_chances(Score::Mate(-1)) < winning_chances(Score::Mate(-12)));
        assert_eq!(
            winning_chances(Score::Mate(12)),
            winning_chances(Score::Mate(10))
        );
    }

    #[test]
    fn people_are_counted_from_lichess_games() {
        // Players rated 1450 and 1720 at 10+0. White's 2. Qh5 gives up 0.6 of a pawn, an
        // inaccuracy; Black's 3... Ke7 walks into a mate in 2, a blunder; the rest are fine.
        let pgn = r#"[Event "Rated rapid game"]
[WhiteElo "1450"]
[BlackElo "1720"]
[TimeControl "600+0"]

1. e4 { [%eval 0.2] } 1... e5 { [%eval 0.2] } 2. Qh5 { [%eval -0.4] } 2... Nc6 { [%eval -0.3] } 3. Bc4 { [%eval -0.2] } 3... Ke7 { [%eval #2] } 1-0

[Event "Rated rapid game"]
[WhiteElo "1500"]
[BlackElo "1500"]
[TimeControl "600+0"]

1. e4 e5 1-0
"#;
        let (tallies, evaluated) = count_people(pgn.as_bytes(), 10, 100);
        assert_eq!(evaluated, 1, "the game without evaluations is left out");
        let white = tallies[&("rapid", 1400)];
        let black = tallies[&("rapid", 1700)];
        assert_eq!((white.moves, white.errors), (3, [1, 0, 0]));
        assert_eq!((black.moves, black.errors), (3, [0, 0, 1]));
        assert_eq!(speed("180+2"), Some("blitz"));
        assert_eq!(speed("15+0"), Some("ultrabullet"));
        assert_eq!(speed("1800+30"), Some("classical"));
        assert_eq!(speed("-"), None);
    }

    #[test]
    fn moves_are_judged_by_how_much_they_lower_the_chances() {
        assert_eq!(classify(0.05), 0);
        assert_eq!(classify(0.1), 1);
        assert_eq!(classify(0.25), 2);
        assert_eq!(classify(0.9), 3);
        let mut tally = Tally::default();
        for kind in [0, 0, 1, 3, 3] {
            tally.add(kind);
        }
        assert_eq!(tally.moves, 5);
        assert_eq!(tally.errors, [1, 0, 2]);
        assert_eq!(tally.per_game(), [8.0, 0.0, 16.0]);
    }
}
