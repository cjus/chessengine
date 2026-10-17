//! Self-play data generation: games of cb2026 against itself, recorded for tuning.
//!
//! The Texel tuner (see [`crate::texel`]) needs many positions, each labelled with the result
//! of the game it came from. This module produces them:
//!
//! 1. Start from an opening position and play a few **random moves**, so games differ.
//! 2. Let the engine play both sides with a small **fixed node count** per move (fast, and the
//!    same on any machine).
//! 3. **Record quiet positions**: not in check, best move not a capture or promotion, no mate
//!    score in sight. The evaluation is meant for quiet positions (the quiescence search
//!    resolves the rest), so those are the ones to tune on.
//! 4. Label every recorded position with the game's result from White's side: 1, ½ or 0.
//!
//! Decided games are cut short (**adjudicated**) once the search sees a mate, and long quiet
//! games are adjudicated as draws once the score stays near zero.
//!
//! Only cb2026's own games are used: no other engine's games or data (`CLAUDE.md` rule 6).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use cb2026_chess::game::Game;
use cb2026_chess::{Color, Move, Position};
use cb2026_engine::{Engine, Limits, Score};

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
pub fn play_game(engine: &mut Engine, start: &Position, nodes: u64) -> Vec<Sample> {
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
                    recorded.push(position.to_fen());
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

    recorded
        .into_iter()
        .map(|fen| Sample { fen, result })
        .collect()
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
    let fen = &settings.openings[index % settings.openings.len()];
    let mut position = Position::from_fen(fen).expect("opening FENs are valid");
    let mut rng = Rng::new(settings.seed.wrapping_add(index as u64));
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

/// Plays `settings.games` games on `settings.threads` threads, calling `on_game` with each
/// game's samples as it finishes (from the calling thread's point of view, one at a time).
pub fn generate(settings: &DatagenSettings, on_game: &(dyn Fn(usize, Vec<Sample>) + Sync)) {
    let next = AtomicUsize::new(0);
    let report = Mutex::new(());
    thread::scope(|scope| {
        for _ in 0..settings.threads.max(1) {
            scope.spawn(|| {
                let mut engine = Engine::new();
                // A small table: games at a few thousand nodes per move barely fill 1 MB.
                engine.set_hash_size(4);
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= settings.games {
                        break;
                    }
                    let start = starting_position(settings, index);
                    let samples = play_game(&mut engine, &start, settings.nodes);
                    let _guard = report.lock().unwrap_or_else(|e| e.into_inner());
                    on_game(index, samples);
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
        let samples = play_game(&mut Engine::new(), &start, 3000);
        assert!(samples.iter().all(|s| s.result == 1.0));
        for sample in &samples {
            let pos = Position::from_fen(&sample.fen).unwrap();
            assert!(!pos.in_check());
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
        };
        let a = starting_position(&settings, 0);
        let b = starting_position(&settings, 1);
        assert_ne!(a.to_fen(), b.to_fen());
        assert_eq!(a.to_fen(), starting_position(&settings, 0).to_fen());
    }
}
