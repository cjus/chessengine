//! Running a match between two engines: many games, in parallel, with live statistics.
//!
//! Each opening is played twice, once with each engine as White, so any advantage the
//! opening gives one colour cancels out. Several games run at once (each worker thread keeps
//! its own pair of engine processes), and with an SPRT the match stops as soon as the result
//! is clear.
//!
//! ## When something goes wrong
//!
//! A test is only as good as the games it counts, so the runner is strict about failures
//! (cjus/chessengine#89):
//!
//! - An engine that crashes or hangs during a game loses it ([`Termination::Failure`]). Its
//!   worker then starts both engines afresh: a crashed process can't play the next game, and
//!   a hung one may still answer the last `go`, with a `bestmove` the next game would take for
//!   its own.
//! - A game that can't start is aborted ([`Outcome::Aborted`]). It is reported, but not
//!   scored: counting it as a draw would let a dead engine add evidence to a test. Its opening
//!   isn't played again, so that opening's pair is left one-sided; aborts should be rare, and
//!   [`MAX_ABORTS_IN_A_ROW`] aborted games in a row on one worker stop the match.
//! - The openings are checked before any game starts, and a worker that can't start its
//!   engines stops the match with an error, instead of leaving it short of games without a
//!   word. So does a worker that panics, in a debug build or a test; a release build aborts
//!   on a panic (`panic = "abort"` in the workspace `Cargo.toml`), which ends the process with
//!   status 134, and `scripts/mac-sprt.sh` reports that as a failed run too.
//! - Once the match stops (a verdict, or another worker's error), a worker finishes its game
//!   and quits: it starts no engines and raises no error after that, so a game that ends badly
//!   after the verdict can't throw the verdict away.

use std::any::Any;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use cb2026_chess::game as rules;
use cb2026_chess::{Color, Position};

use crate::game::{Game, Outcome, Termination, TimeControl, play_game};
use crate::sprt::{Score, Sprt, Verdict};
use crate::uci_client::UciEngine;

/// The opening positions shipped with cb2026 (`tests/data/openings.epd`): 50 balanced
/// positions a few moves into common openings.
pub const DEFAULT_OPENINGS: &str = include_str!("../../../tests/data/openings.epd");

/// Aborted games in a row, on one worker, that stop the match: an engine that keeps failing
/// to start games won't recover by itself.
pub const MAX_ABORTS_IN_A_ROW: usize = 3;

/// Reads opening positions: one per line, the FEN before any `;`; `#` lines are comments.
/// [`check_openings`] says whether they are valid.
pub fn parse_openings(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split(';').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Checks that there are openings and that a game can start from each: a valid position the
/// rules haven't already ended. A bad line in an opening file then stops the match before it
/// starts, not a worker in the middle of it, and an opening that is already a draw can't add
/// draws no engine played.
pub fn check_openings(openings: &[String]) -> Result<(), String> {
    if openings.is_empty() {
        return Err("no opening positions".to_string());
    }
    for (i, fen) in openings.iter().enumerate() {
        let position =
            Position::from_fen(fen).map_err(|e| format!("opening {}, '{fen}': {e}", i + 1))?;
        if rules::Game::from_position(position).outcome().is_some() {
            return Err(format!(
                "opening {}, '{fen}': the game is already over",
                i + 1
            ));
        }
    }
    Ok(())
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

/// One finished game, as reported to the caller. Aborted games are reported too, but leave
/// `score` and `verdict` as they were.
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

impl GameReport {
    /// The engine (0 = first) whose time loss, illegal move or failure ended the game, if one
    /// did.
    pub fn engine_at_fault(&self) -> Option<usize> {
        let color = self.game.at_fault?;
        Some(usize::from(
            (color == Color::White) != self.first_engine_white,
        ))
    }
}

/// One engine's games that ended badly: see [`Problems`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProblemCounts {
    /// Games it lost on time.
    pub time_losses: usize,
    /// Games it lost by an illegal move.
    pub illegal_moves: usize,
    /// Games it lost by crashing or hanging.
    pub failures: usize,
    /// Games aborted, not scored, because it couldn't start them.
    pub aborted: usize,
}

impl fmt::Display for ProblemCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} time losses, {} illegal moves, {} failures, {} aborted games",
            self.time_losses, self.illegal_moves, self.failures, self.aborted
        )
    }
}

/// The games of a match that ended badly, by the engine at fault, for robustness runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Problems {
    /// Each engine's count (0 = first).
    pub counts: [ProblemCounts; 2],
    /// Why each of those games ended, in order.
    pub reasons: Vec<String>,
}

impl Problems {
    /// Counts `report`'s game if it ended badly.
    pub fn record(&mut self, report: &GameReport) {
        let Some(which) = report.engine_at_fault() else {
            return;
        };
        let counts = &mut self.counts[which];
        match (report.game.outcome, report.game.termination) {
            (Outcome::Aborted, _) => counts.aborted += 1,
            (_, Termination::TimeLoss) => counts.time_losses += 1,
            (_, Termination::IllegalMove) => counts.illegal_moves += 1,
            (_, Termination::Failure) => counts.failures += 1,
            (_, Termination::Normal) => return,
        }
        self.reasons.push(report.game.reason.clone());
    }
}

/// Plays the match, calling `on_game` after each game. Returns the final score, or an error
/// if the match couldn't be played in full (see the module docs).
pub fn run_match(
    settings: &MatchSettings,
    on_game: &mut dyn FnMut(&GameReport),
) -> Result<Score, String> {
    check_openings(&settings.openings)?;
    // Check both engines start before launching workers.
    for which in 0..2 {
        start_engine(settings, which)?;
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
            guarded(&stop, || {
                play_games(&settings, total, &next_game, &stop, &sender)
            })
        }));
    }
    drop(sender);

    let mut score = Score::default();
    for (game, first_white) in &results {
        match (game.outcome, first_white) {
            (Outcome::WhiteWins, true) | (Outcome::BlackWins, false) => score.wins += 1,
            (Outcome::WhiteWins, false) | (Outcome::BlackWins, true) => score.losses += 1,
            (Outcome::Draw, _) => score.draws += 1,
            // Not scored: the game never started.
            (Outcome::Aborted, _) => {}
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
    let mut errors: Vec<String> = Vec::new();
    for worker in workers {
        let result = worker
            .join()
            .unwrap_or_else(|panic| Err(panic_message(&*panic)));
        if let Err(e) = result
            && !errors.contains(&e)
        {
            errors.push(e);
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(score)
}

/// Starts engine `which` (0 = first) with its options.
fn start_engine(settings: &MatchSettings, which: usize) -> Result<UciEngine, String> {
    let path = &settings.engines[which];
    UciEngine::start(path, &settings.engine_options(which)).map_err(|e| format!("{path}: {e}"))
}

/// One worker: starts a pair of engines and plays games until there are none left, the match
/// stops, or something goes wrong that starting the engines again can't fix.
fn play_games(
    settings: &MatchSettings,
    total: usize,
    next_game: &AtomicUsize,
    stop: &AtomicBool,
    sender: &mpsc::Sender<(Game, bool)>,
) -> Result<(), String> {
    let mut engines = [start_engine(settings, 0)?, start_engine(settings, 1)?];
    let mut aborts_in_a_row = 0;
    while !stop.load(Ordering::Relaxed) {
        let index = next_game.fetch_add(1, Ordering::Relaxed);
        if index >= total {
            break;
        }
        let opening = &settings.openings[(index / 2) % settings.openings.len()];
        let first_white = index.is_multiple_of(2);
        let [first, second] = &mut engines;
        let pair = if first_white {
            [first, second]
        } else {
            [second, first]
        };
        let game = play_game(pair, opening, settings.time_control);
        let failed = game.termination == Termination::Failure;
        if game.outcome == Outcome::Aborted {
            aborts_in_a_row += 1;
        } else {
            aborts_in_a_row = 0;
        }
        let reason = game.reason.clone();
        if sender.send((game, first_white)).is_err() || stop.load(Ordering::Relaxed) {
            // The match is over: nothing this game did matters to it any more.
            break;
        }
        if aborts_in_a_row >= MAX_ABORTS_IN_A_ROW {
            return Err(format!(
                "{aborts_in_a_row} games in a row couldn't start, the last one {reason}"
            ));
        }
        if failed {
            // A crashed engine can't play the next game, and a hung one may still answer
            // this game's last `go`. Start both afresh, the old ones stopped first.
            drop(engines);
            engines = [start_engine(settings, 0)?, start_engine(settings, 1)?];
        }
    }
    Ok(())
}

/// Runs a worker's `work`, turning a panic into an error, and sets `stop` if it fails, so
/// that the other workers start no more games.
fn guarded(stop: &AtomicBool, work: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let result =
        panic::catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|p| Err(panic_message(&*p)));
    if result.is_err() {
        stop.store(true, Ordering::Relaxed);
    }
    result
}

/// What a caught panic said.
fn panic_message(panic: &(dyn Any + Send)) -> String {
    let text = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message");
    format!("a worker panicked: {text}")
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
        let openings = parse_openings(DEFAULT_OPENINGS);
        assert_eq!(openings.len(), 50);
        assert_eq!(check_openings(&openings), Ok(()));
    }

    #[test]
    fn bad_openings_are_refused() {
        assert!(check_openings(&[]).is_err());
        let malformed = vec![
            cb2026_chess::STARTING_FEN.to_string(),
            "8/8/8 w".to_string(),
        ];
        let e = check_openings(&malformed).unwrap_err();
        assert!(e.starts_with("opening 2, '8/8/8 w'"), "{e}");
        // Fool's mate: White is checkmated, so no game can start. Two bare kings and a
        // spent fifty-move clock are draws already.
        let over =
            vec!["rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3".to_string()];
        let e = check_openings(&over).unwrap_err();
        assert!(e.ends_with("the game is already over"), "{e}");
        for drawn in [
            "8/8/8/8/8/8/8/K6k w - - 0 1",
            "8/8/8/8/8/8/R7/K6k w - - 100 80",
        ] {
            let e = check_openings(&[drawn.to_string()]).unwrap_err();
            assert!(e.ends_with("the game is already over"), "{e}");
        }
    }

    fn report(outcome: Outcome, termination: Termination, at_fault: Option<Color>) -> GameReport {
        GameReport {
            game: Game {
                start_fen: cb2026_chess::STARTING_FEN.to_string(),
                players: ["A".into(), "B".into()],
                moves: Vec::new(),
                outcome,
                reason: format!("{termination:?}"),
                termination,
                at_fault,
            },
            first_engine_white: true,
            score: Score::default(),
            verdict: None,
        }
    }

    #[test]
    fn problems_go_to_the_engine_at_fault() {
        let mut problems = Problems::default();
        let normal = report(Outcome::Draw, Termination::Normal, None);
        assert_eq!(normal.engine_at_fault(), None);
        problems.record(&normal);
        assert_eq!(problems, Problems::default());

        // The first engine, as White, loses on time.
        let time_loss = report(
            Outcome::BlackWins,
            Termination::TimeLoss,
            Some(Color::White),
        );
        assert_eq!(time_loss.engine_at_fault(), Some(0));
        problems.record(&time_loss);
        // The second engine, as White (the first has Black), plays an illegal move.
        let mut illegal = report(
            Outcome::BlackWins,
            Termination::IllegalMove,
            Some(Color::White),
        );
        illegal.first_engine_white = false;
        assert_eq!(illegal.engine_at_fault(), Some(1));
        problems.record(&illegal);
        // The first engine couldn't start a game, as Black and then as White. The old code
        // blamed Black for every aborted game: right for the first by chance, wrong for the
        // second.
        let mut aborted = report(Outcome::Aborted, Termination::Failure, Some(Color::Black));
        aborted.first_engine_white = false;
        assert_eq!(aborted.engine_at_fault(), Some(0));
        problems.record(&aborted);
        let aborted_white = report(Outcome::Aborted, Termination::Failure, Some(Color::White));
        assert_eq!(aborted_white.engine_at_fault(), Some(0));
        problems.record(&aborted_white);
        // The second engine, as Black, crashes mid-game.
        let crash = report(Outcome::WhiteWins, Termination::Failure, Some(Color::Black));
        problems.record(&crash);

        let first = ProblemCounts {
            time_losses: 1,
            aborted: 2,
            ..ProblemCounts::default()
        };
        let second = ProblemCounts {
            illegal_moves: 1,
            failures: 1,
            ..ProblemCounts::default()
        };
        assert_eq!(problems.counts, [first, second]);
        assert_eq!(problems.reasons.len(), 5);
        assert_eq!(
            first.to_string(),
            "1 time losses, 0 illegal moves, 0 failures, 2 aborted games"
        );
    }

    #[test]
    fn a_failed_or_panicking_worker_stops_the_match() {
        let stop = AtomicBool::new(false);
        assert_eq!(guarded(&stop, || Ok(())), Ok(()));
        assert!(!stop.load(Ordering::Relaxed));
        assert_eq!(
            guarded(&stop, || Err("engine gone".to_string())),
            Err("engine gone".to_string())
        );
        assert!(stop.load(Ordering::Relaxed));
        let stop = AtomicBool::new(false);
        let result = guarded(&stop, || panic!("referee bug"));
        assert_eq!(result, Err("a worker panicked: referee bug".to_string()));
        assert!(stop.load(Ordering::Relaxed));
    }
}
