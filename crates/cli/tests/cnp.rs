//! The Chess Node Protocol: every conformance session from the protocol's repository, played
//! against the real `cb2026` binary (Solrac Chess AI #98).
//!
//! The sessions come with the `cnp-conformance` crate ([`cnp_conformance::SESSIONS`]), so a
//! new session there is checked here as soon as the protocol's pin is bumped. Sessions that need
//! a capability cb2026 doesn't offer (a search window) are skipped, and the test says which.

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
            // cb2026 is an engine node: the voice sessions are for voice nodes.
            if !needs.iter().any(|need| need == "voice") {
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
    // Of the engine's sessions, only the window's needs a capability cb2026 doesn't offer.
    assert_eq!(skipped, vec!["window (needs engine.window)".to_string()]);
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
    assert!(version.contains('+'), "{version} has no commit");
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
