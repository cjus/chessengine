//! Self-play data generation: games of cb2026 against itself, recorded for tuning and training.
//!
//! The Texel tuner (see [`crate::texel`]) needs many positions, each labelled with the result
//! of the game it came from; the NNUE trainer also needs the search's score. This module
//! produces them:
//!
//! 1. Start from an opening position and play a few **random moves**, so games differ. Random
//!    moves often leave one side far ahead, so optionally (`max_start_score`) the start is
//!    searched and the random moves are drawn again until it is roughly balanced.
//! 2. Let the engine play both sides with a small **fixed node count** per move (fast, and the
//!    same on any machine).
//! 3. **Record quiet positions**: not in check, best move not a capture or promotion, no mate
//!    score in sight. The evaluation is meant for quiet positions (the quiescence search
//!    resolves the rest), so those are the ones to tune on. Each keeps the search's score and
//!    best move.
//! 4. Label every recorded position with the game's result from White's side: 1, ½ or 0.
//!
//! A [`PlayedGame`] gives its positions as the tuner's text samples ([`PlayedGame::samples`])
//! or as the NNUE trainer's 32-byte records ([`PlayedGame::records`], see
//! [`cb2026_nnue::record`]).
//!
//! Decided games are cut short (**adjudicated**) once the search sees a mate, and long quiet
//! games are adjudicated as draws once the score stays near zero.
//!
//! Only cb2026's own games are used: no other engine's games or data (`CLAUDE.md` rule 6).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use cb2026_chess::game::Game;
use cb2026_chess::{Color, Move, Position};
use cb2026_engine::{Engine, Limits, Network, Score};
use cb2026_nnue::{GameResult, Record};

/// A small, fast pseudo-random number generator (xorshift64*). Seeded per game, so a run can
/// be reproduced exactly.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator from `seed` (any value; 0 is replaced).
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1))
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..n` (`n` > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// How to generate data.
#[derive(Clone, Debug)]
pub struct DatagenSettings {
    /// Games to play.
    pub games: usize,
    /// Nodes the engine searches per move.
    pub nodes: u64,
    /// Random moves played after the opening position.
    pub random_plies: usize,
    /// Games played at the same time.
    pub threads: usize,
    /// Seed for the random moves (each game uses `seed + game number`).
    pub seed: u64,
    /// Opening positions, used in turn.
    pub openings: Vec<String>,
    /// If set, a start (opening plus random moves) whose search score is beyond this many
    /// centipawns is drawn again, so games start roughly balanced. `None` keeps every start.
    pub max_start_score: Option<i32>,
    /// The NNUE network the games are played with: the built-in one by default, so that the
    /// next network learns from the current one's games; `None` for the hand-crafted
    /// evaluation, which played the first network's data and chapter 16's.
    pub network: Option<Arc<Network>>,
}

/// A recorded position: its FEN and the game's result from White's side (1, 0.5 or 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// The position.
    pub fen: String,
    /// The result: 1.0 if White won, 0.5 for a draw, 0.0 if Black won.
    pub result: f64,
}

impl Sample {
    /// The sample as one line of a data file: `FEN;result`.
    pub fn to_line(&self) -> String {
        format!("{};{:.1}", self.fen, self.result)
    }

    /// Reads a line written by [`Sample::to_line`].
    pub fn from_line(line: &str) -> Option<Sample> {
        let (fen, result) = line.rsplit_once(';')?;
        let result: f64 = result.trim().parse().ok()?;
        (0.0..=1.0).contains(&result).then(|| Sample {
            fen: fen.trim().to_string(),
            result,
        })
    }
}

/// A position recorded during a game, with what the search said about it.
#[derive(Clone, Debug)]
pub struct Recorded {
    /// The position.
    pub position: Position,
    /// The search's score in centipawns, from White's side.
    pub score: i32,
    /// The move the search chose.
    pub best_move: Move,
}

/// A finished self-play game: its recorded positions and its result.
#[derive(Clone, Debug)]
pub struct PlayedGame {
    /// The quiet positions, in the order they came up.
    pub positions: Vec<Recorded>,
    /// The result from White's side: 1.0, 0.5 or 0.0.
    pub result: f64,
}

impl PlayedGame {
    /// The positions as the Texel tuner's samples: FEN and result.
    pub fn samples(&self) -> Vec<Sample> {
        self.positions
            .iter()
            .map(|r| Sample {
                fen: r.position.to_fen(),
                result: self.result,
            })
            .collect()
    }

    /// The positions as NNUE training records: the whole position, score, result and best move.
    pub fn records(&self) -> Vec<Record> {
        let result = if self.result > 0.5 {
            GameResult::WhiteWin
        } else if self.result < 0.5 {
            GameResult::BlackWin
        } else {
            GameResult::Draw
        };
        self.positions
            .iter()
            .map(|r| {
                Record::new(&r.position, r.score, result, r.best_move)
                    .expect("a position from a game has at most 32 pieces")
            })
            .collect()
    }
}

/// Games longer than this are adjudicated as draws.
const MAX_PLIES: usize = 400;
/// Draw adjudication: after this many plies...
const DRAW_ADJUDICATION_PLY: usize = 80;
/// ...if the score has stayed within this many centipawns of zero...
const DRAW_ADJUDICATION_SCORE: i32 = 10;
/// ...for this many plies in a row.
const DRAW_ADJUDICATION_LENGTH: usize = 12;
/// Positions with a score beyond this are not recorded (the game is decided).
const RECORD_SCORE_LIMIT: i32 = 2000;

// ANCHOR: play_game
/// Plays one self-play game from `start` and returns its quiet positions with the result.
pub fn play_game(engine: &mut Engine, start: &Position, nodes: u64) -> PlayedGame {
    engine.new_game();
    let stop = AtomicBool::new(false);
    let limits = Limits {
        nodes: Some(nodes),
        ..Limits::default()
    };
    let mut game = Game::from_position(start.clone());
    let mut recorded = Vec::new();
    let mut quiet_plies = 0;

    let result = loop {
        // The rules: is the game over?
        if let Some(outcome) = game.outcome() {
            break outcome.winner().map_or(0.5, winner);
        }
        if game.moves().len() >= MAX_PLIES {
            break 0.5;
        }

        let position = game.position();
        let mut score = None;
        let history = game.history_hashes();
        let reply = engine.search(position, &history, &limits, &stop, &mut |info| {
            score = Some(info.score)
        });
        let Some(mv) = reply.best_move else {
            break 0.5;
        };
        match score {
            // A forced mate: adjudicate.
            Some(Score::Mate(moves)) => {
                let mover = position.side_to_move();
                break winner(if moves > 0 { mover } else { !mover });
            }
            Some(Score::Centipawns(cp)) => {
                if is_quiet(position, mv) && cp.abs() < RECORD_SCORE_LIMIT {
                    // The search scores for the side to move; records keep White's side.
                    let score = match position.side_to_move() {
                        Color::White => cp,
                        Color::Black => -cp,
                    };
                    recorded.push(Recorded {
                        position: position.clone(),
                        score,
                        best_move: mv,
                    });
                }
                quiet_plies = if cp.abs() <= DRAW_ADJUDICATION_SCORE {
                    quiet_plies + 1
                } else {
                    0
                };
                if game.moves().len() >= DRAW_ADJUDICATION_PLY
                    && quiet_plies >= DRAW_ADJUDICATION_LENGTH
                {
                    break 0.5;
                }
            }
            None => {}
        }
        game.play(mv).expect("the engine plays legal moves");
    };

    PlayedGame {
        positions: recorded,
        result,
    }
}
// ANCHOR_END: play_game

/// The result for a win by `color`, from White's side.
fn winner(color: Color) -> f64 {
    match color {
        Color::White => 1.0,
        Color::Black => 0.0,
    }
}

/// Whether `pos`, with `best` to be played, is worth recording: not in check, and the best
/// move neither captures nor promotes.
fn is_quiet(pos: &Position, best: Move) -> bool {
    !pos.in_check() && !best.is_capture() && best.promotion().is_none()
}

/// The starting position for game `index`: an opening, then random legal moves (stopping
/// early if a random move ends the game).
pub fn starting_position(settings: &DatagenSettings, index: usize) -> Position {
    random_start(settings, index, 0)
}

/// Try `attempt` at a start for game `index`. Try 0 is [`starting_position`]'s; each later try
/// draws other random moves from the same opening.
fn random_start(settings: &DatagenSettings, index: usize, attempt: u64) -> Position {
    let fen = &settings.openings[index % settings.openings.len()];
    let mut position = Position::from_fen(fen).expect("opening FENs are valid");
    let seed = settings
        .seed
        .wrapping_add(index as u64)
        .wrapping_add(attempt.wrapping_mul(ATTEMPT_SEED_STEP));
    let mut rng = Rng::new(seed);
    for _ in 0..settings.random_plies {
        let moves = position.legal_moves();
        if moves.is_empty() {
            break;
        }
        let mv = moves[rng.below(moves.len())];
        let mut next = position.clone();
        next.make_move(mv);
        if next.legal_moves().is_empty() {
            break;
        }
        position = next;
    }
    position
}

/// How many times a start's random moves are drawn before giving up on a balanced one.
const START_ATTEMPTS: u64 = 20;
/// Spaces out the random seeds of successive tries, so they don't overlap other games' seeds.
const ATTEMPT_SEED_STEP: u64 = 0x9e37_79b9_7f4a_7c15;

// ANCHOR: balanced_start
/// The start for game `index`, roughly balanced if `settings.max_start_score` is set: each try
/// is searched like a move of the game, and the first that scores within the limit is used. If
/// none of the `START_ATTEMPTS` (20) tries does, the most balanced is.
pub fn balanced_start(engine: &mut Engine, settings: &DatagenSettings, index: usize) -> Position {
    let Some(limit) = settings.max_start_score else {
        return starting_position(settings, index);
    };
    let mut best: Option<(i32, Position)> = None;
    for attempt in 0..START_ATTEMPTS {
        let start = random_start(settings, index, attempt);
        let imbalance = start_score(engine, &start, settings.nodes);
        if imbalance <= limit {
            return start;
        }
        if best.as_ref().is_none_or(|(b, _)| imbalance < *b) {
            best = Some((imbalance, start));
        }
    }
    best.expect("at least one try").1
}
// ANCHOR_END: balanced_start

/// How far from level a start is: the size of its search score in centipawns, with a mate
/// counted as the largest imbalance.
fn start_score(engine: &mut Engine, start: &Position, nodes: u64) -> i32 {
    engine.new_game();
    let limits = Limits {
        nodes: Some(nodes),
        ..Limits::default()
    };
    let mut score = None;
    engine.search(start, &[], &limits, &AtomicBool::new(false), &mut |info| {
        score = Some(info.score)
    });
    match score {
        Some(Score::Centipawns(cp)) => cp.abs(),
        Some(Score::Mate(_)) | None => i32::MAX,
    }
}

/// Plays `settings.games` games on `settings.threads` threads, calling `on_game` with each
/// game as it finishes (from the calling thread's point of view, one at a time).
pub fn generate(settings: &DatagenSettings, on_game: &(dyn Fn(usize, PlayedGame) + Sync)) {
    let next = AtomicUsize::new(0);
    let report = Mutex::new(());
    thread::scope(|scope| {
        for _ in 0..settings.threads.max(1) {
            scope.spawn(|| {
                let mut engine = Engine::new();
                engine.network = settings.network.clone();
                // A small table: games at a few thousand nodes per move barely fill 1 MB.
                engine.set_hash_size(4);
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= settings.games {
                        break;
                    }
                    let start = balanced_start(&mut engine, settings, index);
                    let game = play_game(&mut engine, &start, settings.nodes);
                    let _guard = report.lock().unwrap_or_else(|e| e.into_inner());
                    on_game(index, game);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_round_trip_through_lines() {
        let sample = Sample {
            fen: cb2026_chess::STARTING_FEN.to_string(),
            result: 0.5,
        };
        assert_eq!(Sample::from_line(&sample.to_line()), Some(sample));
        assert_eq!(Sample::from_line("nonsense"), None);
        assert_eq!(Sample::from_line("8/8/8/8/8/8/8/8 w - - 0 1;2.0"), None);
    }

    #[test]
    fn rng_is_reproducible() {
        let a: Vec<u64> = (0..5)
            .map(|_| 0)
            .scan(Rng::new(7), |r, _| Some(r.next_u64()))
            .collect();
        let b: Vec<u64> = (0..5)
            .map(|_| 0)
            .scan(Rng::new(7), |r, _| Some(r.next_u64()))
            .collect();
        assert_eq!(a, b);
        assert!(a.windows(2).all(|w| w[0] != w[1]));
    }

    #[test]
    fn plays_a_short_game_and_labels_its_positions() {
        // King and queen against king: White mates quickly.
        let start = Position::from_fen("8/8/8/4k3/8/8/8/3QK3 w - - 0 1").unwrap();
        let game = play_game(&mut Engine::new(), &start, 3000);
        assert_eq!(game.result, 1.0);
        for sample in &game.samples() {
            let pos = Position::from_fen(&sample.fen).unwrap();
            assert!(!pos.in_check());
            assert_eq!(sample.result, 1.0);
        }
    }

    #[test]
    fn records_keep_the_score_from_whites_side_and_the_best_move() {
        let start = Position::from_fen(cb2026_chess::STARTING_FEN).unwrap();
        let game = play_game(&mut Engine::new(), &start, 2000);
        let records = game.records();
        assert_eq!(records.len(), game.positions.len());
        assert!(!records.is_empty());
        for (record, recorded) in records.iter().zip(&game.positions) {
            assert_eq!(i32::from(record.score()), recorded.score);
            assert_eq!(record.best_move(), recorded.best_move);
            assert!(
                recorded
                    .position
                    .legal_moves()
                    .contains(&recorded.best_move)
            );
            assert_eq!(record.result().for_white(), game.result as f32);
            assert_eq!(record.side_to_move(), recorded.position.side_to_move());
        }
    }

    fn settings_from(opening: &str, max_start_score: Option<i32>) -> DatagenSettings {
        DatagenSettings {
            games: 1,
            nodes: 1000,
            random_plies: 8,
            threads: 1,
            seed: 3,
            openings: vec![opening.to_string()],
            max_start_score,
            network: None,
        }
    }

    #[test]
    fn without_a_limit_the_start_is_the_first_random_one() {
        let settings = settings_from(cb2026_chess::STARTING_FEN, None);
        for index in 0..3 {
            let start = balanced_start(&mut Engine::new(), &settings, index);
            assert_eq!(start.to_fen(), starting_position(&settings, index).to_fen());
        }
    }

    #[test]
    fn with_a_limit_the_start_is_balanced() {
        let settings = settings_from(cb2026_chess::STARTING_FEN, Some(100));
        let mut engine = Engine::new();
        for index in 0..4 {
            let start = balanced_start(&mut engine, &settings, index);
            assert!(
                start_score(&mut engine, &start, settings.nodes) <= 100,
                "{}",
                start.to_fen()
            );
        }
    }

    #[test]
    fn when_no_start_is_balanced_the_most_balanced_is_used() {
        // Black has no queen: no random moves make that level, so every try fails the limit.
        let opening = "rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let settings = settings_from(opening, Some(50));
        let mut engine = Engine::new();
        let start = balanced_start(&mut engine, &settings, 0);
        let chosen = start_score(&mut engine, &start, settings.nodes);
        assert!(chosen > 50);
        for attempt in 0..START_ATTEMPTS {
            let other = random_start(&settings, 0, attempt);
            assert!(chosen <= start_score(&mut engine, &other, settings.nodes));
        }
    }

    #[test]
    fn random_openings_differ_but_are_reproducible() {
        let settings = DatagenSettings {
            games: 2,
            nodes: 1000,
            random_plies: 6,
            threads: 1,
            seed: 1,
            openings: vec![cb2026_chess::STARTING_FEN.to_string()],
            max_start_score: None,
            network: None,
        };
        let a = starting_position(&settings, 0);
        let b = starting_position(&settings, 1);
        assert_ne!(a.to_fen(), b.to_fen());
        assert_eq!(a.to_fen(), starting_position(&settings, 0).to_fen());
    }
}
