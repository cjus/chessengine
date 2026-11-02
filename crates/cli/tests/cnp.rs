//! The Chess Node Protocol: every conformance session from the protocol's repository, played
//! against the real `cb2026` binary (Solrac Chess AI #98).
//!
//! The sessions come with the `cnp-conformance` crate ([`cnp_conformance::SESSIONS`]), so a
//! new session there is checked here as soon as the protocol's pin is bumped. Sessions that need
//! an engine capability cb2026 doesn't offer (a search window), or that are for a node without
//! one it does (pondering, the search tree, following the root), are skipped, and the test says
//! which.

use std::path::PathBuf;
use std::time::Duration;

use cnp_conformance::runner::{Outcome, capabilities, missing, run_session};
use cnp_conformance::{SESSIONS, session};

/// How long one step of a session may wait. Generous: the tests run unoptimised builds.
const STEP_TIMEOUT: Duration = Duration::from_secs(60);

fn node(args: &[&str]) -> Vec<String> {
    std::iter::once(env!("CARGO_BIN_EXE_cb2026"))
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect()
}

/// Whether the source is in a git repository, asked the way `crates/engine/build.rs` asks: then
/// the build script recorded the commit. A source archive has no repository (a release tarball,
/// or a remote-build snapshot, which leaves out `.git`), so it builds the plain version.
fn in_git_repository() -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Whether a capability a session needs (`engine.ponder`, `!engine.ponder`, `games.read`) is
/// in the engine domain.
fn is_engine(need: &str) -> bool {
    need.trim_start_matches('!').split('.').next() == Some("engine")
}

fn run_all(command: &[String]) -> (Vec<String>, Vec<String>) {
    let mut sessions = session::load(&[PathBuf::from(SESSIONS)]).expect("the sessions load");
    assert!(sessions.len() >= 40, "only {} sessions", sessions.len());
    for session in &mut sessions {
        session.timeout = session.timeout.max(STEP_TIMEOUT);
    }
    let offered = capabilities(command, STEP_TIMEOUT).expect("cb2026 answers initialize");
    let (mut failed, mut skipped) = (Vec::new(), Vec::new());
    for session in &sessions {
        let needs = missing(session, &offered);
        if !needs.is_empty() {
            // cb2026 is an engine node: the other domains' sessions (voice, and Solrac's own
            // games, preferences and backups) are for other nodes.
            if needs.iter().all(|need| is_engine(need)) {
                skipped.push(format!("{} (needs {})", session.name, needs.join(", ")));
            }
            continue;
        }
        if let Outcome::Failed(why) = run_session(session, command) {
            failed.push(format!("{}: {why}", session.path.display()));
        }
    }
    (failed, skipped)
}

#[test]
fn cb2026_passes_every_conformance_session_that_applies() {
    let (failed, skipped) = run_all(&node(&["--cnp"]));
    assert!(failed.is_empty(), "failed:\n{}", failed.join("\n\n"));
    // Of the engine's sessions, only the window's needs a capability cb2026 doesn't offer, and
    // four are for nodes without capabilities it has: pondering, an evaluation's quiet line
    // and piece values, the search tree, and following the root.
    assert_eq!(
        skipped,
        vec![
            "evaluate-quiet-pieces-unsupported (needs !engine.quiet, !engine.pieces)".to_string(),
            "ponder-unsupported (needs !engine.ponder)".to_string(),
            "root-unsupported (needs !engine.root)".to_string(),
            "tree-unsupported (needs !engine.tree)".to_string(),
            "window (needs engine.window)".to_string(),
        ]
    );
}

#[test]
fn a_json_first_line_starts_the_protocol_without_a_flag() {
    let sessions = session::load(&[PathBuf::from(SESSIONS).join("lifecycle/initialize.jsonl")])
        .expect("the session loads");
    assert_eq!(run_session(&sessions[0], &node(&[])), Outcome::Passed);
}

#[test]
fn initialize_and_version_name_the_commit_cb2026_was_built_from() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let version = cb2026_engine::build_version();
    if in_git_repository() {
        assert!(version.contains('+'), "{version} has no commit");
    } else {
        assert_eq!(version, cb2026_engine::VERSION);
    }
    let printed = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .arg("--version")
        .output()
        .expect("cb2026 runs");
    assert_eq!(
        String::from_utf8_lossy(&printed.stdout).trim(),
        format!("cb2026 {version}")
    );

    let mut node = Command::new(env!("CARGO_BIN_EXE_cb2026"))
        .arg("--cnp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("cb2026 starts");
    let mut stdin = node.stdin.take().expect("its input");
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocol":"1.0","client":{{"name":"test","version":"0"}}}}}}"#
    )
    .expect("initialize is sent");
    drop(stdin);
    let mut answer = String::new();
    BufReader::new(node.stdout.take().expect("its output"))
        .read_line(&mut answer)
        .expect("cb2026 answers");
    node.wait().expect("cb2026 ends when its input does");
    assert!(
        answer.contains(&format!(r#""version":"{version}""#)),
        "{answer}"
    );
}
