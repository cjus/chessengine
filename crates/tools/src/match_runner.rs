//! Running a match between two engines: many games, in parallel, with live statistics.
//!
//! Each opening is played twice, once with each engine as White, so any advantage the
//! opening gives one colour cancels out. Several games run at once (each worker thread keeps
//! its own pair of engine processes), and with an SPRT the match stops as soon as the result
//! is clear.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use crate::game::{Game, Outcome, TimeControl, play_game};
use crate::sprt::{Score, Sprt, Verdict};
use crate::uci_client::UciEngine;

/// Reads opening positions: one per line, the FEN before any `;`; `#` lines are comments.
pub fn parse_openings(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split(';').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// What to play.
#[derive(Clone, Debug)]
pub struct MatchSettings {
    /// Paths to the two engines; results are from the first one's point of view.
    pub engines: [String; 2],
    /// UCI options for both engines.
    pub options: Vec<(String, String)>,
    /// Extra UCI options for the first engine only (for example `Threads`, to test how the
    /// engine scales against a single-threaded copy of itself).
    pub first_options: Vec<(String, String)>,
    /// How long each move may take.
    pub time_control: TimeControl,
    /// Opening positions (FEN), played in order, each twice with colours swapped.
    pub openings: Vec<String>,
    /// The most games to play (rounded up to whole pairs).
    pub max_games: usize,
    /// Games played at the same time.
    pub concurrency: usize,
    /// Stop early when this test reaches a verdict.
    pub sprt: Option<Sprt>,
}

impl MatchSettings {
    /// The options for engine `which` (0 = first): the shared ones, then its own.
    pub fn engine_options(&self, which: usize) -> Vec<(String, String)> {
        let mut options = self.options.clone();
        if which == 0 {
            options.extend(self.first_options.iter().cloned());
        }
        options
    }
}

/// One finished game, as reported to the caller.
#[derive(Clone, Debug)]
pub struct GameReport {
    /// The game.
    pub game: Game,
    /// Whether the first engine had White.
    pub first_engine_white: bool,
    /// The running score after this game.
    pub score: Score,
    /// The SPRT verdict after this game, if a test is running.
    pub verdict: Option<Verdict>,
}

/// Plays the match, calling `on_game` after each game. Returns the final score.
pub fn run_match(
    settings: &MatchSettings,
    on_game: &mut dyn FnMut(&GameReport),
) -> Result<Score, String> {
    if settings.openings.is_empty() {
        return Err("no opening positions".to_string());
    }
    // Check both engines start before launching workers.
    for which in 0..2 {
        let path = &settings.engines[which];
        UciEngine::start(path, &settings.engine_options(which))
            .map_err(|e| format!("{path}: {e}"))?;
    }

    let total = settings.max_games.div_ceil(2) * 2;
    let next_game = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, results) = mpsc::channel::<(Game, bool)>();

    let mut workers = Vec::new();
    for _ in 0..settings.concurrency.max(1) {
        let settings = settings.clone();
        let next_game = Arc::clone(&next_game);
        let stop = Arc::clone(&stop);
        let sender = sender.clone();
        workers.push(thread::spawn(move || {
            let start = |which: usize| {
                UciEngine::start(&settings.engines[which], &settings.engine_options(which))
            };
            let (Ok(mut first), Ok(mut second)) = (start(0), start(1)) else {
                return;
            };
            while !stop.load(Ordering::Relaxed) {
                let index = next_game.fetch_add(1, Ordering::Relaxed);
                if index >= total {
                    break;
                }
                let opening = &settings.openings[(index / 2) % settings.openings.len()];
                let first_white = index.is_multiple_of(2);
                let game = if first_white {
                    play_game([&mut first, &mut second], opening, settings.time_control)
                } else {
                    play_game([&mut second, &mut first], opening, settings.time_control)
                };
                if sender.send((game, first_white)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(sender);

    let mut score = Score::default();
    for (game, first_white) in &results {
        let first_won = match game.outcome {
            Outcome::Draw => None,
            Outcome::WhiteWins => Some(first_white),
            Outcome::BlackWins => Some(!first_white),
        };
        match first_won {
            Some(true) => score.wins += 1,
            Some(false) => score.losses += 1,
            None => score.draws += 1,
        }
        let verdict = settings.sprt.map(|sprt| sprt.verdict(&score));
        on_game(&GameReport {
            game,
            first_engine_white: first_white,
            score,
            verdict,
        });
        if matches!(verdict, Some(Verdict::Pass | Verdict::Fail)) {
            // The test is decided the moment the LLR crosses a bound. Games still being
            // played could pull it back across, so they are finished but not counted.
            stop.store(true, Ordering::Relaxed);
            break;
        }
    }
    // Wait for the workers to finish the games they had started.
    for _ in results {}
    for worker in workers {
        let _ = worker.join();
    }
    Ok(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_openings() {
        let text = "# comment\n\nrnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1 ; e4\n";
        assert_eq!(
            parse_openings(text),
            vec!["rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"]
        );
    }

    #[test]
    fn the_shipped_openings_are_valid() {
        let openings = parse_openings(include_str!("../../../tests/data/openings.epd"));
        assert_eq!(openings.len(), 50);
        for fen in openings {
            let pos = cb2026_chess::Position::from_fen(&fen).unwrap();
            assert!(!pos.legal_moves().is_empty(), "{fen}");
        }
    }
}
