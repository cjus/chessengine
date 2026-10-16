//! The match runner when engines fail (cjus/chessengine#89). Both sides are the fake engine
//! (`src/bin/fake-uci.rs`), which plays its first legal move and crashes when told to.

use std::path::PathBuf;
use std::process::Command;

use cb2026_tools::game::{Outcome, Termination, TimeControl};
use cb2026_tools::match_runner::{GameReport, MatchSettings, Problems, run_match};
use cb2026_tools::sprt::{Score, Sprt, Verdict};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-uci");

/// An empty directory of a test's own for the fake's counter files, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "cb2026-match-failures-{}-{test}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the temporary directory is writable");
        Scratch(dir)
    }

    /// The fake's `Counter` option.
    fn counter(&self) -> String {
        self.0.join("fake").display().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `games` games between two fakes, one at a time; the first fake gets `first_options`.
fn settings(games: usize, first_options: &[(&str, String)]) -> MatchSettings {
    MatchSettings {
        engines: [FAKE.to_string(), FAKE.to_string()],
        options: Vec::new(),
        first_options: first_options
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect(),
        time_control: TimeControl::Nodes(1),
        openings: vec![cb2026_chess::STARTING_FEN.to_string()],
        max_games: games,
        concurrency: 1,
        sprt: None,
    }
}

/// Plays the match: its result, the problems and every report.
fn play(settings: &MatchSettings) -> (Result<Score, String>, Problems, Vec<GameReport>) {
    let mut problems = Problems::default();
    let mut reports = Vec::new();
    let result = run_match(settings, &mut |report| {
        problems.record(report);
        reports.push(report.clone());
    });
    (result, problems, reports)
}

#[test]
fn two_fakes_play_normal_games() {
    let (result, problems, reports) = play(&settings(4, &[]));
    assert_eq!(result.unwrap().games(), 4);
    assert_eq!(problems, Problems::default());
    assert!(
        reports
            .iter()
            .all(|r| r.game.termination == Termination::Normal)
    );
}

#[test]
fn a_crash_loses_the_game_and_the_next_games_are_played() {
    // The first fake crashes at its third move of the match, in game 1 (it has White).
    let scratch = Scratch::new("crash");
    let options = [("FailGo", "3".to_string()), ("Counter", scratch.counter())];
    let (result, problems, reports) = play(&settings(4, &options));
    let score = result.unwrap();
    assert_eq!(reports[0].game.termination, Termination::Failure);
    assert_eq!(reports[0].game.outcome, Outcome::BlackWins);
    assert_eq!(problems.counts[0].failures, 1);
    // Without a restart the dead fake couldn't start games 2 to 4, and they were scored as
    // draws. Now it is started again and they are played.
    assert!(
        reports[1..]
            .iter()
            .all(|r| r.game.termination == Termination::Normal),
        "{reports:?}"
    );
    assert_eq!(score.games(), 4);
    assert!(score.losses >= 1);
    assert_eq!(problems.counts[0].aborted + problems.counts[1].aborted, 0);
}

#[test]
fn a_game_that_cannot_start_is_aborted_and_not_scored() {
    // The first fake dies on its second `ucinewgame`: game 2, where it has Black.
    let scratch = Scratch::new("abort");
    let options = [
        ("FailNewGame", "2".to_string()),
        ("Counter", scratch.counter()),
    ];
    let (result, problems, reports) = play(&settings(4, &options));
    let score = result.unwrap();
    assert_eq!(reports.len(), 4);
    assert_eq!(reports[1].game.outcome, Outcome::Aborted);
    assert_eq!(reports[1].engine_at_fault(), Some(0));
    assert!(reports[1].game.to_string().contains("[Result \"*\"]"));
    // Reported, not scored; and the games after it are played.
    assert_eq!(score.games(), 3);
    assert_eq!(problems.counts[0].aborted, 1);
    assert_eq!(problems.counts[1], Default::default());
    assert_eq!(reports[3].game.termination, Termination::Normal);
}

#[test]
fn aborted_games_in_a_row_stop_the_match() {
    let options = [("FailNewGame", "1+".to_string())];
    let (result, problems, reports) = play(&settings(10, &options));
    let e = result.unwrap_err();
    assert!(e.starts_with("3 games in a row couldn't start"), "{e}");
    assert_eq!(reports.len(), 3);
    assert_eq!(problems.counts[0].aborted, 3);
}

#[test]
fn an_engine_that_will_not_start_again_stops_the_match() {
    // The first fake crashes in game 1, then can't be started again: its first two starts
    // are the runner's check and the worker's.
    let scratch = Scratch::new("restart");
    let options = [
        ("FailGo", "3".to_string()),
        ("FailStart", "3+".to_string()),
        ("Counter", scratch.counter()),
    ];
    let (result, _, reports) = play(&settings(4, &options));
    let e = result.unwrap_err();
    assert!(e.starts_with(FAKE), "{e}");
    assert_eq!(reports.len(), 1);
}

#[test]
fn several_workers_stop_at_the_verdict() {
    // The first fake crashes at each of its moves, so it loses every game and the SPRT soon
    // fails. The games the other workers are still playing then end badly too; that must not
    // throw the verdict away.
    let mut settings = settings(1000, &[("FailGo", "1+".to_string())]);
    settings.concurrency = 3;
    settings.sprt = Some(Sprt::new(0.0, 5.0));
    let (result, problems, reports) = play(&settings);
    let score = result.unwrap();
    assert_eq!(reports.last().unwrap().verdict, Some(Verdict::Fail));
    assert_eq!((score.wins, score.draws), (0, 0));
    // Only the games up to the verdict are counted, and each is the first fake's failure.
    assert_eq!(score.games() as usize, reports.len());
    assert_eq!(problems.counts[0].failures, reports.len());
    assert!(reports.len() < 1000);
}

#[test]
fn a_missing_engine_or_a_bad_opening_stops_the_match_before_it_starts() {
    let mut missing = settings(2, &[]);
    missing.engines[1] = "/no/such/engine".to_string();
    let (result, _, reports) = play(&missing);
    assert!(result.unwrap_err().starts_with("/no/such/engine"));
    assert!(reports.is_empty());

    let mut bad_opening = settings(2, &[]);
    bad_opening.openings.push("not a position".to_string());
    let (result, _, reports) = play(&bad_opening);
    assert!(
        result
            .unwrap_err()
            .starts_with("opening 2, 'not a position'")
    );
    assert!(reports.is_empty());
}

#[test]
fn the_match_tool_fails_when_no_game_is_scored() {
    let openings = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/data/openings.epd");
    let output = Command::new(env!("CARGO_BIN_EXE_match"))
        .args(["--engine", FAKE, "--engine", FAKE, "--games", "0"])
        .args(["--openings", openings, "--sprt", "0", "5"])
        .output()
        .expect("the match tool runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("SPRT: no games were scored"), "{stdout}");
}
